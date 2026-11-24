import { describe, expect, it } from 'vitest';

import { CM6_BUNDLE_JS } from '../cm6Bundle';

// A feature that grows the page raises this in the same commit with a decision; a trim lowers it.
// Allow 571 more characters to color Python calls to annotated and declared types.
const BUDGET_CHARS = 663_523;

describe('the editor bundle size', () => {
  it('given the committed CodeMirror bundle, when its length is read, then it is within the budget', () => {
    expect(CM6_BUNDLE_JS.length).toBeLessThanOrEqual(BUDGET_CHARS);
  });
});
