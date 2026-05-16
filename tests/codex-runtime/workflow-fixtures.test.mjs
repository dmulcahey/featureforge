import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import {
  REPO_ROOT,
  readUtf8,
} from './helpers/markdown-test-helpers.mjs';

const FIXTURE_ROOT = path.join(REPO_ROOT, 'tests/codex-runtime/fixtures/workflow-artifacts');

const SPEC_FIXTURES = [
  'specs/2026-01-22-document-review-system-design.md',
  'specs/2026-01-22-document-review-system-design-v2.md',
  'specs/2026-02-19-visual-brainstorming-refactor-design.md',
  'specs/2026-03-11-zero-dep-brainstorm-server-design.md',
  'specs/2026-03-22-runtime-integration-hardening-design.md',
];

const PLAN_FIXTURES = [
  'plans/2026-01-22-document-review-system.md',
  'plans/2026-02-19-visual-brainstorming-refactor.md',
  'plans/2026-03-11-zero-dep-brainstorm-server.md',
  'plans/2026-03-22-runtime-integration-hardening.md',
];

const STALE_PATH_PLAN_FIXTURE = 'plans/2026-01-22-document-review-system-stale-path.md';
const REQUIRED_HARNESS_AWARE_DOWNSTREAM_PHASES = [
  'final_review_pending',
  'qa_pending',
  'document_release_pending',
  'ready_for_branch_completion',
];
const ACTIVE_DOC_PATHS = [
  'RELEASE-NOTES.md',
  'TODOS.md',
  'docs/README.codex.md',
  'docs/README.copilot.md',
  'docs/testing.md',
  'docs/test-suite-enhancement-plan.md',
  'tests/evals/README.md',
];
const TASK8_HARNESS_MATRIX_FIXTURES = [
  'harness/pivot-required-status.json',
  'harness/handoff-required-status.json',
  'harness/candidate-execution-contract.md',
  'harness/candidate-evaluation-report.md',
  'harness/candidate-execution-handoff.md',
  'harness/stale-execution-contract.md',
  'harness/stale-evaluation-report.md',
  'harness/repo-state-drift-status.json',
  'harness/partial-authoritative-mutation-status.json',
  'harness/dependency-index-mismatch-status.json',
  'harness/dependency-index-clean.json',
  'harness/dependency-index-stale.json',
  'harness/dependency-index-malformed.json',
  'harness/non-harness-review-artifact.md',
  'harness/indexed-final-review-artifact.md',
  'harness/indexed-browser-qa-artifact.md',
  'harness/indexed-release-doc-artifact.md',
  'harness/retention-prunable-stale-artifact.md',
  'harness/retention-active-authoritative-artifact.md',
];
const DEPENDENCY_INDEX_STATUS_FIXTURES = [
  'harness/dependency-index-mismatch-status.json',
  'harness/dependency-index-clean.json',
  'harness/dependency-index-stale.json',
  'harness/dependency-index-malformed.json',
];
const REQUIRED_DEPENDENCY_INDEX_STATES = new Set([
  'healthy',
  'missing',
  'malformed',
  'inconsistent',
  'recovering',
]);
const OBSERVABILITY_SEAM_EVENT_KINDS_FIXTURE = 'harness/observability-seam-event-kinds.json';
const REQUIRED_ADVANCED_RUNTIME_EVENT_KINDS = [
  'authoritative_mutation_recorded',
  'blocked_state_cleared',
  'blocked_state_entered',
  'integrity_mismatch_detected',
  'ordering_gap_detected',
  'partial_mutation_recovered',
  'replay_accepted',
  'replay_conflict',
  'repo_state_drift_detected',
  'repo_state_reconciled',
  'write_authority_conflict',
  'write_authority_reclaimed',
];

function retiredProductName() {
  const readme = readUtf8(path.join(REPO_ROOT, 'README.md'));
  const provenanceLine = readme
    .split('\n')
    .find((line) => line.startsWith('FeatureForge began from upstream '));
  assert.ok(provenanceLine, 'README.md should keep the provenance attribution line');
  const match = provenanceLine.match(/upstream ([A-Za-z]+):/);
  assert.ok(match, 'README.md provenance line should expose the retired product name');
  return match[1].toLowerCase();
}

const RETIRED_PRODUCT = retiredProductName();
const LEGACY_ACTIVE_DOC_PATTERN = new RegExp(
  `${RETIRED_PRODUCT}|using_${RETIRED_PRODUCT}_skill|using-${RETIRED_PRODUCT}|\\.${RETIRED_PRODUCT}|${RETIRED_PRODUCT.toUpperCase()}_`,
  'i',
);

function getExactHeaderLine(content, label) {
  const escaped = label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = content.match(new RegExp(`^\\*\\*${escaped}:\\*\\* .+$`, 'm'));
  return match ? match[0] : null;
}

test('all workflow fixture files exist', () => {
  for (const relPath of [...SPEC_FIXTURES, ...PLAN_FIXTURES, STALE_PATH_PLAN_FIXTURE]) {
    const content = readUtf8(path.join(FIXTURE_ROOT, relPath));
    assert.match(content, /^# /m, `${relPath} should keep a markdown title`);
    assert.notEqual(
      getExactHeaderLine(content, 'Workflow State'),
      null,
      `${relPath} should keep a parseable workflow-state header`,
    );
  }
});

test('task 8 harness fixture matrix preserves parseable contract content', () => {
  for (const relPath of TASK8_HARNESS_MATRIX_FIXTURES) {
    const content = readUtf8(path.join(FIXTURE_ROOT, relPath));
    if (relPath.endsWith('.json')) {
      const payload = JSON.parse(content);
      assert.equal(typeof payload, 'object', `${relPath} should contain a JSON object`);
      assert.notEqual(payload, null, `${relPath} should not be null`);
      assert.notEqual(
        payload.status ?? payload.dependency_index_state ?? payload.harness_phase,
        undefined,
        `${relPath} should preserve at least one harness status surface`,
      );
      continue;
    }

    assert.match(content, /^# /m, `${relPath} should keep a markdown title`);
    assert.match(
      content,
      /\*\*[A-Za-z0-9 _-]+:\*\* /,
      `${relPath} should keep parseable markdown metadata headers`,
    );
  }
});

test('task 8 dependency-index fixtures pin minimum status key and canonical state vocabulary', () => {
  for (const relPath of DEPENDENCY_INDEX_STATUS_FIXTURES) {
    const filePath = path.join(FIXTURE_ROOT, relPath);
    const payload = JSON.parse(readUtf8(filePath));
    assert.equal(typeof payload, 'object', `${relPath} should contain a JSON object`);
    assert.notEqual(payload, null, `${relPath} should not be null`);
    assert.equal(
      typeof payload.dependency_index_state,
      'string',
      `${relPath} should include dependency_index_state`,
    );
    assert.equal(
      REQUIRED_DEPENDENCY_INDEX_STATES.has(payload.dependency_index_state),
      true,
      `${relPath} should use canonical dependency_index_state vocabulary`,
    );
  }
});

test('observability seam fixture pins advanced runtime event_kind vocabulary', () => {
  const fixturePath = path.join(FIXTURE_ROOT, OBSERVABILITY_SEAM_EVENT_KINDS_FIXTURE);
  const payload = JSON.parse(readUtf8(fixturePath));
  assert.equal(Array.isArray(payload.observability_event_examples), true);

  const observedEventKinds = payload.observability_event_examples
    .map((entry) => entry?.event_kind)
    .filter((eventKind) => typeof eventKind === 'string' && eventKind.length > 0);
  const missingEventKinds = REQUIRED_ADVANCED_RUNTIME_EVENT_KINDS.filter(
    (eventKind) => !observedEventKinds.includes(eventKind),
  );

  assert.deepEqual(
    missingEventKinds,
    [],
    'observability seam fixture should include advanced runtime-stable event_kind literals',
  );
});

test('spec fixtures carry the required workflow headers', () => {
  for (const relPath of SPEC_FIXTURES) {
    const content = readUtf8(path.join(FIXTURE_ROOT, relPath));
    assert.equal(getExactHeaderLine(content, 'Workflow State'), '**Workflow State:** CEO Approved', `${relPath} should use the exact approved-spec workflow state line`);
    assert.equal(getExactHeaderLine(content, 'Spec Revision'), '**Spec Revision:** 1', `${relPath} should use the exact spec revision line`);
    assert.equal(getExactHeaderLine(content, 'Last Reviewed By'), '**Last Reviewed By:** plan-ceo-review', `${relPath} should use the exact spec reviewer line`);
  }
});

test('plan fixtures carry the required workflow headers', () => {
  const happyPathSpecs = [
    'specs/2026-01-22-document-review-system-design.md',
    'specs/2026-02-19-visual-brainstorming-refactor-design.md',
    'specs/2026-03-11-zero-dep-brainstorm-server-design.md',
    'specs/2026-03-22-runtime-integration-hardening-design.md',
  ];
  for (const [index, relPath] of PLAN_FIXTURES.entries()) {
    const content = readUtf8(path.join(FIXTURE_ROOT, relPath));
    assert.equal(getExactHeaderLine(content, 'Workflow State'), '**Workflow State:** Engineering Approved', `${relPath} should use the exact approved-plan workflow state line`);
    assert.equal(
      getExactHeaderLine(content, 'Source Spec'),
      `**Source Spec:** \`tests/codex-runtime/fixtures/workflow-artifacts/${happyPathSpecs[index]}\``,
      `${relPath} should point at the matching spec fixture`,
    );
    assert.equal(getExactHeaderLine(content, 'Source Spec Revision'), '**Source Spec Revision:** 1', `${relPath} should use the exact source revision line`);
    assert.equal(getExactHeaderLine(content, 'Last Reviewed By'), '**Last Reviewed By:** plan-eng-review', `${relPath} should use the exact plan reviewer line`);
  }
});

test('stale-path plan fixture preserves the source-spec path mismatch case', () => {
  const content = readUtf8(path.join(FIXTURE_ROOT, STALE_PATH_PLAN_FIXTURE));
  assert.equal(getExactHeaderLine(content, 'Workflow State'), '**Workflow State:** Engineering Approved');
  assert.equal(
    getExactHeaderLine(content, 'Source Spec'),
    '**Source Spec:** `tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-01-22-document-review-system-design.md`',
  );
  assert.equal(getExactHeaderLine(content, 'Source Spec Revision'), '**Source Spec Revision:** 1');
  assert.equal(getExactHeaderLine(content, 'Last Reviewed By'), '**Last Reviewed By:** plan-eng-review');
});

test('full-contract route-time fixture preserves plan revision, execution mode, and canonical task shape', () => {
  const content = readUtf8(path.join(FIXTURE_ROOT, 'plans/2026-03-22-runtime-integration-hardening.md'));
  assert.equal(getExactHeaderLine(content, 'Plan Revision'), '**Plan Revision:** 1');
  assert.equal(getExactHeaderLine(content, 'Execution Mode'), '**Execution Mode:** none');
  assert.match(content, /## Requirement Coverage Matrix/);
  assert.match(content, /## Task 1: Harden route-time workflow validation/);
  assert.match(content, /\*\*Files:\*\*/);
});

test('fixture README documents provenance and intent', () => {
  const content = readUtf8(path.join(FIXTURE_ROOT, 'README.md'));
  assert.match(content, /108c0e8/);
  assert.match(content, /ce106d0/);
  assert.match(content, /header contract/i);
  assert.match(content, /stale source-spec path/i);
  assert.match(content, /full approved-plan-contract pair/i);
});

test('workflow fixture bundle pins downstream phase and operator metadata semantics', () => {
  const pivot = JSON.parse(readUtf8(path.join(FIXTURE_ROOT, 'harness/pivot-required-status.json')));
  assert.equal(pivot.harness_phase, 'execution_preflight');
  assert.equal(pivot.status, 'pivot_required');
  assert.deepEqual(pivot.reason_codes, ['retry_budget_exhausted']);
  assert.equal(pivot.next_action, 'pivot_plan');

  const handoff = JSON.parse(readUtf8(path.join(FIXTURE_ROOT, 'harness/handoff-required-status.json')));
  assert.equal(handoff.harness_phase, 'handoff_required');
  assert.equal(handoff.status, 'waiting_for_downstream_gates');
  assert.deepEqual(handoff.reason_codes, ['handoff_submission_pending']);
  assert.equal(handoff.next_action, 'submit_handoff');

  const candidateHandoff = readUtf8(path.join(FIXTURE_ROOT, 'harness/candidate-execution-handoff.md'));
  assert.equal(getExactHeaderLine(candidateHandoff, 'Harness Phase'), '**Harness Phase:** handoff_required');
  assert.equal(getExactHeaderLine(candidateHandoff, 'Next Action'), '**Next Action:** final_review_pending');

  const candidateEvaluation = readUtf8(path.join(FIXTURE_ROOT, 'harness/candidate-evaluation-report.md'));
  assert.equal(getExactHeaderLine(candidateEvaluation, 'Evaluator Kind'), '**Evaluator Kind:** spec_compliance');
  assert.equal(getExactHeaderLine(candidateEvaluation, 'Verdict'), '**Verdict:** pass');

  const indexedFinalReview = readUtf8(path.join(FIXTURE_ROOT, 'harness/indexed-final-review-artifact.md'));
  const indexedQa = readUtf8(path.join(FIXTURE_ROOT, 'harness/indexed-browser-qa-artifact.md'));
  const indexedReleaseDocs = readUtf8(path.join(FIXTURE_ROOT, 'harness/indexed-release-doc-artifact.md'));
  assert.equal(getExactHeaderLine(indexedFinalReview, 'Artifact Kind'), '**Artifact Kind:** final_review');
  assert.equal(getExactHeaderLine(indexedQa, 'Artifact Kind'), '**Artifact Kind:** browser_qa');
  assert.equal(getExactHeaderLine(indexedReleaseDocs, 'Artifact Kind'), '**Artifact Kind:** release_docs');
  assert.equal(getExactHeaderLine(indexedFinalReview, 'Indexed By Harness'), '**Indexed By Harness:** true');
  assert.equal(getExactHeaderLine(indexedQa, 'Indexed By Harness'), '**Indexed By Harness:** true');
  assert.equal(getExactHeaderLine(indexedReleaseDocs, 'Indexed By Harness'), '**Indexed By Harness:** true');

  const fixtureReadme = readUtf8(path.join(FIXTURE_ROOT, 'README.md'));
  for (const phase of REQUIRED_HARNESS_AWARE_DOWNSTREAM_PHASES) {
    assert.match(
      fixtureReadme,
      new RegExp(`\\\`${phase}\\\``),
      `fixture README should pin downstream phase ${phase}`,
    );
  }
  assert.match(fixtureReadme, /downstream freshness\/status surfaces/i);
  assert.match(fixtureReadme, /evaluator-kind visibility/i);
  assert.match(fixtureReadme, /next_action/);
  assert.match(fixtureReadme, /reason_codes/);
  assert.match(fixtureReadme, /write-authority metadata/i);
  assert.match(fixtureReadme, /write-authority conflict/i);
  assert.doesNotMatch(fixtureReadme, /review_blocked/);
});

test('active docs reserve legacy attribution to the README provenance section only', () => {
  const readme = readUtf8(path.join(REPO_ROOT, 'README.md'));
  const provenanceStart = readme.indexOf('## Provenance');
  const nextSectionStart = readme.indexOf('## How It Works');

  assert.notEqual(provenanceStart, -1, 'README.md should define a Provenance section');
  assert.notEqual(nextSectionStart, -1, 'README.md should define the next section after Provenance');
  assert.ok(nextSectionStart > provenanceStart, 'README.md should keep Provenance before How It Works');

  const readmeOutsideProvenance = `${readme.slice(0, provenanceStart)}${readme.slice(nextSectionStart)}`;
  assert.doesNotMatch(readmeOutsideProvenance, LEGACY_ACTIVE_DOC_PATTERN, 'README.md should keep legacy naming inside the Provenance section only');

  for (const relativePath of ACTIVE_DOC_PATHS) {
    const content = readUtf8(path.join(REPO_ROOT, relativePath));
    assert.doesNotMatch(content, LEGACY_ACTIVE_DOC_PATTERN, `${relativePath} should not mention the legacy product in active docs`);
  }
});

test('repo-local config and historical docs use the featureforge archive layout', () => {
  const repoConfig = readUtf8(path.join(REPO_ROOT, '.featureforge/config.yaml'));
  assert.notEqual(repoConfig.trim().length, 0, '.featureforge/config.yaml should be non-empty');
  assert.match(repoConfig, /update_check:/);
  assert.throws(
    () => readUtf8(path.join(REPO_ROOT, `.${RETIRED_PRODUCT}/config.yaml`)),
    /ENOENT/,
    `.${RETIRED_PRODUCT}/config.yaml should be removed from the active repo`,
  );

  const archiveRoot = path.join(REPO_ROOT, 'docs/archive', RETIRED_PRODUCT);
  const archivedSpec = readUtf8(path.join(archiveRoot, 'specs/2026-03-22-runtime-integration-hardening-design.md'));
  const archivedPlan = readUtf8(path.join(archiveRoot, 'plans/2026-03-22-runtime-integration-hardening.md'));
  const archivedEvidence = readUtf8(path.join(archiveRoot, 'execution-evidence/2026-03-22-runtime-integration-hardening-r1-evidence.md'));
  assert.match(archivedSpec, /^# /m, `archived historical specs should live under docs/archive/${RETIRED_PRODUCT}/specs`);
  assert.match(archivedPlan, /^# /m, `archived historical plans should live under docs/archive/${RETIRED_PRODUCT}/plans`);
  assert.match(archivedEvidence, /^# /m, `archived historical execution evidence should live under docs/archive/${RETIRED_PRODUCT}/execution-evidence`);
  assert.throws(
    () => readUtf8(path.join(REPO_ROOT, 'docs', RETIRED_PRODUCT, 'README.md')),
    /ENOENT/,
    `docs/${RETIRED_PRODUCT} should be removed after the archive move`,
  );
});
