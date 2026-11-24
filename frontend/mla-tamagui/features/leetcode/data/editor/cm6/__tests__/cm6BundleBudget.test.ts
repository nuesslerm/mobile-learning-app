import { describe, expect, it } from 'vitest';

import { CM6_BUNDLE_JS } from '../cm6Bundle';

// A feature that grows the page raises this in the same commit with a decision; a trim lowers it.
// Matching the canvas line spacing and comment colors adds 59 characters.
const BUDGET_CHARS = 662_952;

describe('the editor bundle size', () => {
  it('given the committed CodeMirror bundle, when its length is read, then it is within the budget', () => {
    expect(CM6_BUNDLE_JS.length).toBeLessThanOrEqual(BUDGET_CHARS);
  });
});
