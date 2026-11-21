import { describe, expect, it } from 'vitest';

import { harnessSchema } from '../models';

describe('the problem harness', () => {
  it('given a design or a round-trip harness with its names, when it is parsed, then it is accepted as written', () => {
    expect(harnessSchema.parse({ kind: 'design', className: 'Trie' })).toEqual({ kind: 'design', className: 'Trie' });
    const codec = { kind: 'roundTrip', className: 'Codec', encode: 'serialize', decode: 'deserialize' };
    expect(harnessSchema.parse(codec)).toEqual(codec);
  });

  it('given a round-trip harness without its decode name or a kind nobody runs, when it is parsed, then it is refused', () => {
    expect(harnessSchema.safeParse({ kind: 'roundTrip', className: 'Codec', encode: 'encode' }).success).toBe(false);
    expect(harnessSchema.safeParse({ kind: 'script', className: 'Trie' }).success).toBe(false);
  });
});
