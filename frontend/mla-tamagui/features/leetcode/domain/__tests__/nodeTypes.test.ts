import { describe, expect, it } from 'vitest';

import { isHarnessOnly, nodeTypeSchema } from '../models';

describe('the node kinds of a problem’s values', () => {
  it('given every node kind, when it is asked whether it is harness-only, then only the cycle position is, and an unknown key is not', () => {
    const harnessOnly = nodeTypeSchema.options.filter((type) => isHarnessOnly(type));

    expect(harnessOnly).toEqual(['ListNode_cycle_pos']);
    expect(isHarnessOnly(undefined)).toBe(false);
  });

  it('given the graph and cycle position kinds, when they are parsed, then they are accepted', () => {
    expect(nodeTypeSchema.parse('GraphNode')).toBe('GraphNode');
    expect(nodeTypeSchema.parse('ListNode_cycle_pos')).toBe('ListNode_cycle_pos');
  });
});
