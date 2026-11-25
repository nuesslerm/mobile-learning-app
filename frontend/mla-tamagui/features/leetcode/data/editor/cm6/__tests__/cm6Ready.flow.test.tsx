import { act, waitFor } from '@lib/flow-test-harness';
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

describe('editor input profiling', () => {
  it('Given profiling is enabled, When a user edits the page, Then timing crosses without document content', async () => {
    const page = await mountPage({ init: true, profileInput: true });
    await waitFor(() => expect(page.frame()?.document.querySelector('.cm-content')).toBeTruthy());

    await act(async () => {
      page.frame()?.__editorDebug__?.simulateUser({ kind: 'type', text: 'private code' });
    });

    await waitFor(() => expect(page.inputTimings).toHaveLength(1));
    expect(page.inputTimings[0]).toEqual({
      type: 'inputTiming',
      inputToUpdateMs: null,
      updateToFrameMs: expect.any(Number),
      updates: 1,
      sentAtMs: expect.any(Number),
    });
    page.rpc.close();
  });
});
