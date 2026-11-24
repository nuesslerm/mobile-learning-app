import { classify, errorRanges } from '@lib/code-editor-api';
import { parseFor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { paletteItem, PYTHON_PALETTE } from '../palette';

describe('the Python palette', () => {
  it('given the palette, when its chips are listed, then they are if, for, while, def and return, once each', () => {
    expect(PYTHON_PALETTE.map((item) => item.id)).toEqual(['if', 'for', 'while', 'def', 'return']);
    expect(PYTHON_PALETTE.map((item) => paletteItem(item.id))).toEqual([...PYTHON_PALETTE]);
  });

  it.each(PYTHON_PALETTE.map((item) => [item.id, item.snippet] as const))('given the %s snippet, when it is parsed, then it is exactly one statement with no error', (id, snippet) => {
    const root = parseFor(snippet, 'python');

    expect(root.children?.length).toBe(1);
    expect(errorRanges(root)).toEqual([]);
    expect(['simple', 'compound']).toContain(classify('python', root.children?.[0] as NonNullable<typeof root.children>[number]));
    expect(snippet.startsWith(id)).toBe(true);
  });

  it.each(PYTHON_PALETTE.filter((item) => item.id !== 'return').map((item) => [item.id, item.snippet] as const))('given the %s snippet, when its lines are read, then the body is a pass one level in', (_id, snippet) => {
    const lines = snippet.split('\n');

    expect(lines).toHaveLength(2);
    expect(lines[0]?.endsWith(':')).toBe(true);
    expect(lines[1]).toBe('    pass');
  });
});
