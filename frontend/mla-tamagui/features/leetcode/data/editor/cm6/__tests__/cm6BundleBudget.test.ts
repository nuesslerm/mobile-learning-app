import { describe, expect, it } from 'vitest';

import { CM6_BUNDLE_JS } from '../cm6Bundle';

// A feature that grows the page raises this in the same commit with a decision; a trim lowers it.
// Reporting when Undo is available adds 179 characters.
const BUDGET_CHARS = 662_893;

describe('the editor bundle size', () => {
  it('given the committed CodeMirror bundle, when its length is read, then it is within the budget', () => {
    expect(CM6_BUNDLE_JS.length).toBeLessThanOrEqual(BUDGET_CHARS);
  });
});
