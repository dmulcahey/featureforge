# Debugging and TDD Examples

This companion holds examples and rationale for `systematic-debugging` and `test-driven-development`. The top-level skills carry the mandatory laws; this file only expands technique.

## Root-Cause Tracing

When a failure crosses several components, instrument each boundary once, run the failing scenario, and identify the first bad transition before proposing a fix.

```text
input boundary -> component output -> next component input -> failure point
```

For deep stack failures, trace the bad value backward until you find where it was created or accepted. Fix that source rather than adding a guard at the symptom unless defense in depth is also justified.

## Failed Hypotheses

If one minimal hypothesis test fails, update the hypothesis from the evidence. If three fix attempts fail or each attempt exposes new coupling elsewhere, stop and question the architecture before continuing.

## TDD Example

```typescript
test('rejects empty email', async () => {
  const result = await submitForm({ email: '' });
  expect(result.error).toBe('Email required');
});
```

Expected RED:

```text
FAIL: expected 'Email required', got undefined
```

Minimal GREEN:

```typescript
function submitForm(data: FormData) {
  if (!data.email?.trim()) {
    return { error: 'Email required' };
  }
}
```

## Common Rationalizations

Avoid these patterns:
- "I'll test after."
- "This is too simple to test."
- "Keep the implementation as a reference."
- "One more fix attempt" after repeated failed hypotheses.
- "The mock was called, so the behavior is tested."

Tests should exercise real behavior, not mock internals, unless isolation is unavoidable.
