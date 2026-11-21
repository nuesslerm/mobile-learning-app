import { describe, expect, it } from 'vitest';

import { applyVoiceEdit } from '../applyVoiceEdit';
import type { EditAlternative } from '@lib/voice-edit-dsl';

function makeAlt(commands: EditAlternative['commands']): EditAlternative {
  return {
    id: 'alt-1',
    label: 'Test',
    summary: 'Summary',
    confidence: 0.9,
    commands,
  };
}

describe('applyVoiceEdit', () => {
  it('given a replaceRange command when applied then modifies text and computes diff range', () => {
    const original = 'function add(a, b) { return a + b; }';
    const alt = makeAlt([
      { type: 'replaceRange', start: 9, end: 12, newText: 'sum' },
    ]);
    const result = applyVoiceEdit(original, alt);
    expect(result.modifiedText).toBe('function sum(a, b) { return a + b; }');
    expect(result.diffRange.start).toBe(9);
    expect(result.diffRange.end).toBe(12);
  });

  it('given an insertAt command when applied then highlights inserted region', () => {
    const original = 'console.log();';
    const alt = makeAlt([{ type: 'insertAt', position: 12, text: '"hello"' }]);
    const result = applyVoiceEdit(original, alt);
    expect(result.modifiedText).toBe('console.log("hello");');
    expect(result.diffRange.start).toBe(12);
    expect(result.diffRange.end).toBe(19);
  });

  it('given an empty command list when applied then returns unchanged diff range', () => {
    const original = 'const x = 1;';
    const alt = makeAlt([]);
    const result = applyVoiceEdit(original, alt);
    expect(result.modifiedText).toBe(original);
    expect(result.diffRange.start).toBe(original.length);
    expect(result.diffRange.end).toBe(original.length);
  });

  it('given a full replacement when applied then updates entire text and diff range', () => {
    const original = 'old';
    const alt = makeAlt([{ type: 'replaceRange', start: 0, end: 3, newText: 'new' }]);
    const result = applyVoiceEdit(original, alt);
    expect(result.modifiedText).toBe('new');
    expect(result.diffRange.start).toBe(0);
    expect(result.diffRange.end).toBe(3);
  });
});
