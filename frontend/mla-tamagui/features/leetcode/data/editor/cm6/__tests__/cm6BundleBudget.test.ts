import { describe, expect, it } from 'vitest';

import { CM6_BUNDLE_JS } from '../cm6Bundle';

// A feature that grows the page raises this in the same commit with a decision; a trim lowers it.
// Allow 433 more characters for the opt-in input timing probe.
const BUDGET_CHARS = 663_956;

describe('the editor bundle size', () => {
  it('given the committed CodeMirror bundle, when its length is read, then it is within the budget', () => {
    expect(CM6_BUNDLE_JS.length).toBeLessThanOrEqual(BUDGET_CHARS);
  });
});
