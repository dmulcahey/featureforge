import { estimateCostUsd, writeEvalRecord } from './eval-observability.mjs';

function getEvalModel(env) {
  return env.OPENAI_EVAL_MODEL || env.EVAL_MODEL || null;
}

function isEnabledValue(value) {
  return value === '1' || String(value).toLowerCase() === 'true';
}

export function evalsEnabled(env = process.env) {
  return isEnabledValue(env.FEATUREFORGE_RUN_EVALS)
    || isEnabledValue(env.RUN_EVALS)
    || isEnabledValue(env.EVALS);
}

export function requireEvalEnv(env = process.env) {
  if (!evalsEnabled(env)) {
    return { enabled: false, reason: 'evaluator disabled; set FEATUREFORGE_RUN_EVALS=1, RUN_EVALS=1, or EVALS=1' };
  }

  if (!env.OPENAI_API_KEY) {
    return { enabled: false, reason: 'OPENAI_API_KEY missing' };
  }

  if (!getEvalModel(env)) {
    return { enabled: false, reason: 'OPENAI_EVAL_MODEL or EVAL_MODEL missing' };
  }

  return { enabled: true };
}

export async function runJsonJudgeEval({ name, system, prompt }, env = process.env) {
  const gate = requireEvalEnv(env);
  if (!gate.enabled) {
    return {
      name,
      skipped: true,
      passed: false,
      summary: gate.reason,
      judge_result: { skipped: true, reason: gate.reason },
      reason: gate.reason,
    };
  }

  const startedAt = Date.now();
  const apiBase = env.OPENAI_BASE_URL || 'https://api.openai.com/v1';
  const model = getEvalModel(env);
  const response = await fetch(`${apiBase}/responses`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      authorization: `Bearer ${env.OPENAI_API_KEY}`,
    },
    body: JSON.stringify({
      model,
      input: [
        { role: 'system', content: [{ type: 'input_text', text: system }] },
        { role: 'user', content: [{ type: 'input_text', text: prompt }] },
      ],
    }),
  });

  const body = await response.json();
  if (!response.ok) {
    throw new Error(`Eval request failed: ${response.status} ${JSON.stringify(body)}`);
  }

  const outputText = body.output_text || body.output?.map((item) => item?.content?.map((part) => part.text || '').join('')).join('') || '';
  const parsed = JSON.parse(outputText);
  const usage = body.usage || {};
  const record = {
    ...parsed,
    name,
    passed: Boolean(parsed.passed),
    summary: parsed.summary ?? null,
    transcript: outputText,
    judge_result: parsed,
    usage,
    elapsed_ms: Date.now() - startedAt,
    cost_usd: estimateCostUsd(usage),
    model,
    recorded_at: new Date().toISOString(),
  };
  record.record_path = writeEvalRecord(record, env);
  return record;
}
