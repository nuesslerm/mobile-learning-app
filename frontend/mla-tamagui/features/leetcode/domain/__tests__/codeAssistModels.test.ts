import { describe, expect, it } from 'vitest';

import { toolCallSchema } from '../codeAssistModels';

describe('toolCallSchema', () => {
  it('validates replaceRange', () => {
    const result = toolCallSchema.safeParse({
      tool: 'replaceRange',
      params: { from: 0, to: 5, text: 'hello' },
    });
    expect(result.success).toBe(true);
  });

  it('rejects text > 200 chars', () => {
    const result = toolCallSchema.safeParse({
      tool: 'replaceRange',
      params: { from: 0, to: 5, text: 'x'.repeat(201) },
    });
    expect(result.success).toBe(false);
  });
});
