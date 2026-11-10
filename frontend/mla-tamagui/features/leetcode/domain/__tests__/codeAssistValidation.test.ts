import { describe, expect, it } from 'vitest';

import type { ToolCall } from '../codeAssistModels';
import { validateToolCall } from '../codeAssistValidation';

describe('validateToolCall', () => {
  it('validates replaceRange within bounds', () => {
    const tool: ToolCall = {
      tool: 'replaceRange',
      params: { from: 0, to: 5, text: 'hi' },
    };
    expect(validateToolCall('hello world', tool)).toBe(true);
  });

  it('rejects replaceRange out of bounds', () => {
    const tool: ToolCall = {
      tool: 'replaceRange',
      params: { from: 0, to: 100, text: 'hi' },
    };
    expect(validateToolCall('hi', tool)).toBe(false);
  });

  it('rejects renameSymbol collision', () => {
    const tool: ToolCall = {
      tool: 'renameSymbol',
      params: { oldName: 'x', newName: 'y' },
    };
    expect(validateToolCall('const y = 1', tool)).toBe(false);
  });
});
