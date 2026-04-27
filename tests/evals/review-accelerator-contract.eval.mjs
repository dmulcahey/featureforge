import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { evalsEnabled, requireEvalEnv, runJsonJudgeEval } from './helpers/openai-judge.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, '../..');

const gate = requireEvalEnv();
const testFn = evalsEnabled() && gate.enabled
  ? test
  : (name, fn) => test(name, { skip: gate.reason }, fn);

function readRepoFile(relPath) {
  return fs.readFileSync(path.join(REPO_ROOT, relPath), 'utf8');
}

testFn('review accelerator contract preserves explicit human authority and fail-closed boundaries', async () => {
  const ceoSkill = readRepoFile('skills/plan-ceo-review/SKILL.md');
  const engSkill = readRepoFile('skills/plan-eng-review/SKILL.md');
  const engAcceleratedPrompt = readRepoFile('skills/plan-eng-review/accelerated-reviewer-prompt.md');
  const packetContract = readRepoFile('review/review-accelerator-packet-contract.md');
  const readme = readRepoFile('README.md');

  const result = await runJsonJudgeEval({
    name: 'review-accelerator-contract',
    system: 'You evaluate whether workflow instructions for an AI coding agent clearly preserve explicit human approval authority. Respond with JSON only.',
    prompt: `Evaluate whether this branch's generated review workflow docs clearly enforce the accelerated-review contract.

Baseline inputs:
- generated CEO review skill doc from this branch
- generated ENG review skill doc from this branch
- README excerpts from this branch

Required checks:
1. Only an explicit user request for \`accelerated\` or \`accelerator\` mode enables accelerated review.
2. Ambiguous wording alone, heuristics, sticky state, or agent choice do not activate accelerated review.
3. Human approval remains required per section.
4. The system cannot automatically write CEO Approved or Engineering Approved as part of accelerated review.
5. Only the main review agent may write authoritative artifacts or apply approved patches.
6. Persisted-packet stale or regenerate language is present for fingerprint mismatch or equivalent source-change detection.
7. Accelerated ENG packets preserve the normal engineering hard-fail law for weak task contracts, non-deterministic \`Done when\`, missing required spec references, broad or under-specified task scopes, and avoidable duplicate implementations.
8. Accelerated ENG packet validation fails closed when the required analyze-plan boolean snapshot or reuse assessment is missing or contradicts the normal \`plan-eng-review\` approval gate.
9. Accelerated ENG findings for concrete contract failures use deterministic repair-packet fields tied to the violated task field or canonical \`DONE_WHEN_N\` / \`CONSTRAINT_N\` obligation, including \`Finding ID\`, \`Severity\`, \`Task\`, \`Violated Field or Obligation\`, \`Evidence\`, \`Required Fix\`, and \`Hard Fail\`.

Respond with JSON only:
{"passed": true|false, "summary": "one sentence", "evidence": ["..."]}

CEO skill doc:
${ceoSkill}

ENG skill doc:
${engSkill}

ENG accelerated reviewer prompt:
${engAcceleratedPrompt}

Review accelerator packet contract:
${packetContract}

README excerpt:
${readme}`,
  });

  assert.equal(result.passed, true, JSON.stringify(result.judge_result));
});
