import { describe, expect, it } from 'vitest';

import {
  parseLenientJson,
  repairJsonControlChars,
} from '../src/use-cases/repairJson';

describe('repairJsonControlChars', () => {
  it('escapes raw line breaks inside string literals and nothing else', () => {
    // Given a planner reply whose newText carries raw newlines
    const raw = '{"a":"        while left < right:\n            pass","b":1}\n';
    // When repaired
    const fixed = repairJsonControlChars(raw);
    // Then the in-string newline is escaped and the trailing one outside is kept
    expect(fixed).toBe(
      '{"a":"        while left < right:\\n            pass","b":1}\n'
    );
    expect(JSON.parse(fixed)).toEqual({
      a: '        while left < right:\n            pass',
      b: 1,
    });
  });

  it('is the identity on valid JSON, including escaped quotes and backslashes', () => {
    // Given valid JSON with tricky escapes
    const valid = '{"t":"say \\"hi\\" \\\\ done\\n","n":[1,2]}';
    // When repaired
    // Then it is unchanged
    expect(repairJsonControlChars(valid)).toBe(valid);
  });

  it('parseLenientJson recovers a reply with raw tabs and newlines', () => {
    // Given raw tab and newline inside a string
    const raw = '{"x":"a\tb\nc"}';
    // When parsed leniently
    // Then the value is intact
    expect(parseLenientJson(raw)).toEqual({ x: 'a\tb\nc' });
  });

  it('parseLenientJson rethrows for JSON that is broken beyond control characters', () => {
    // Given a truncated document
    // When parsed leniently
    // Then the error propagates
    expect(() => parseLenientJson('{"x":"a"')).toThrow();
  });
});
