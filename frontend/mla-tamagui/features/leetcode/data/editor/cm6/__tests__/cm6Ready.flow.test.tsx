import { describe, expect, it } from 'vitest';

import { mountPage } from './support/mountEditorPage';

describe('the page announcing itself', () => {
  it('given the real page loads in the twin, when ready crosses, then it carries an integer evalMs', async () => {
    // When the real page is mounted and announces itself
    const page = await mountPage({ init: false });

    // Then the message it posted carries the time its bundle took to evaluate, a whole number of milliseconds
    expect(page.readyMessages.length).toBeGreaterThan(0);
    for (const message of page.readyMessages) {
      const { evalMs } = message as { evalMs: unknown };
      expect(Number.isInteger(evalMs)).toBe(true);
      expect(evalMs as number).toBeGreaterThanOrEqual(0);
    }
    page.rpc.close();
  });
});
