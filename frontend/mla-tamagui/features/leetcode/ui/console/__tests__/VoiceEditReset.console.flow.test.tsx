import { act, createFakeAudioRecorder, createFakeVoiceEditTransport, routerHarness, screen, setStackHeadersRendered, userEvent, waitFor, within } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../../tests/flow/app';
import type { VoiceEditTransportPort } from '../../..';
import { useCodeHistoryStore } from '../../../state/codeHistoryStore';
import { useResetCodeRequestStore } from '../../../state/resetCodeRequestStore';
import { fakeGateway, resetConsoleTwin } from './consoleTwinSupport';

const SEEN_EDIT = {
  id: 'alt-1',
  label: 'Declare seen',
  summary: 'Inserts seen after the signature.',
  confidence: 1,
  commands: [{ type: 'insertAfterLine', line: 3, text: '        seen = {}' }],
};

beforeEach(() => {
  resetConsoleTwin('two-sum');
  useCodeHistoryStore.setState({ snapshots: [] });
});
afterEach(() => setStackHeadersRendered(false));

describe('ProblemEditorScreen flow with the console (voice edit and reset)', () => {
  it('given an applied voice edit, when the code is reset through the header menu, then no edit mark is drawn or held and the toggle is gone', async () => {
    // Given a voice edit that landed and is highlighted, with the console on
    const r = routerHarness();
    setStackHeadersRendered(true);
    const voice = createFakeVoiceEditTransport({ transcript: 'declare seen', alternatives: [SEEN_EDIT] }) as unknown as VoiceEditTransportPort;
    const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: { gateway: fakeGateway(), voice, audio: createFakeAudioRecorder(), flags: { aiEdit: true } } } });
    const editor = await session.editor();
    const starter = editor.text();
    await userEvent.click(screen.getByTestId('ProblemEditorTextEditStartButton'));
    await userEvent.type(await screen.findByTestId('ProblemEditorVoiceReviewSheet-TranscriptInput'), 'declare seen');
    await userEvent.click(screen.getByTestId('ProblemEditorVoiceReviewSheet-Send'));
    await waitFor(() => expect(editor.diffRanges()).toEqual(['seen = {}']));

    // When the person resets the code from the header menu and confirms (the flow router does not mount the reset route's dialog, whose Reset button calls the request's onConfirm)
    await screen.findByTestId('ProblemEditorDiffToggle');
    await userEvent.click(screen.getByTestId('ProblemEditorMenuButton'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorMenuButtonContent')).toBeTruthy());
    await userEvent.click(within(screen.getByTestId('ProblemEditorMenuButtonContent')).getByTestId('ProblemEditorMenuResetCode'));
    await waitFor(() => expect(r.lastPush()).toBe('/leetcode/two-sum/reset-code'));
    await act(async () => useResetCodeRequestStore.getState().payload?.onConfirm());

    // Then the starter is back, nothing is highlighted and the toggle is gone
    await waitFor(() => expect(editor.text()).toBe(starter));
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.diffRanges()).toEqual([]);
    await waitFor(() => expect(screen.queryByTestId('ProblemEditorDiffToggle')).toBeNull());
  }, 60_000);
});
