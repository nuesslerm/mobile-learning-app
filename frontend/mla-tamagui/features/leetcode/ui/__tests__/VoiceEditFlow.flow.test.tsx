import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { describe, expect, it, vi } from 'vitest';

import {
  type LeetCodeGatewayPort,
  type LeetCodePorts,
  type RunResponse,
  type VoiceEditTransportPort,
} from '../..';



// The wheel picker's scroll-to-select is real-DOM mechanics jsdom cannot run;
// its rows tap straight into onValueChanged, which is the screen-facing seam.

const ACCEPTED_RUN: RunResponse = {
  source: 'backend-run',
  kind: 'test',
  state: 'SUCCESS',
  requestId: 'flow-request',
  run_success: true,
  status_msg: 'Accepted',
  total_correct: 5,
  total_testcases: 5,
};

function fakeGateway(): LeetCodeGatewayPort {
  return {
    userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
    test: vi.fn(async () => ({ ...ACCEPTED_RUN, interpret_id: 'flow-interpret' })),
    submit: vi.fn(async () => ACCEPTED_RUN),
    poll: vi.fn(async () => ACCEPTED_RUN),
    runLocal: vi.fn(async () => ACCEPTED_RUN),
    format: vi.fn(async (p: { lang: string; code: string }) => `# formatted\n${p.code}`),
    loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
  };
}

async function renderVoiceEditor(ports: LeetCodePorts) {
  return app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
}

describe('ProblemEditorScreen flow (voice edit)', () => {
  it('given the problem editor with an injected voice pipeline, when the user records, stops and picks the second alternative, then the edit is applied and the diff range is set', async () => {
    const audio = createFakeAudioRecorder();
    const voice = createFakeVoiceEditTransport({
      transcript: 'return the two indices that add up to the target',
      alternatives: [
        {
          id: 'alt-1',
          label: 'Return the zero-based pair',
          summary: 'Returns [0, 1].',
          confidence: 0.6,
          commands: [{ type: 'insertAt', position: 89, text: 'return [0, 1]' }],
        },
        {
          id: 'alt-2',
          label: 'Return the pair at one and two',
          summary: 'Returns [1, 2].',
          confidence: 0.5,
          commands: [{ type: 'insertAt', position: 89, text: 'return [1, 2]' }],
        },
      ],
    }) as unknown as VoiceEditTransportPort;
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice, audio });

    const editor = await session.editor();
    const starter = editor.text();

    await userEvent.click(screen.getByTestId('ProblemEditorVoiceStartButton'));

    await waitFor(() =>
      expect(screen.getByTestId('ProblemEditorVoiceToolbar-Waveform')).toBeTruthy()
    );
    await act(async () => {
      audio.emitLevel(0.9);
    });
    await waitFor(() => {
      const bar = screen.getByTestId('ProblemEditorVoiceToolbar-Waveform-Bar-1');
      expect(parseFloat(bar.style.height)).toBeGreaterThan(6);
    });

    await userEvent.click(screen.getByTestId('ProblemEditorVoiceToolbar-Send'));

    await waitFor(() =>
      expect(screen.getByTestId('ProblemEditorVoiceReviewSheet')).toBeTruthy()
    );
    const transcriptInput = screen.getByTestId('ProblemEditorVoiceReviewSheet-TranscriptInput');
    await waitFor(() => {
      expect((transcriptInput as HTMLTextAreaElement).value).toBe(
        'return the two indices that add up to the target'
      );
    });

    await userEvent.click(screen.getByTestId('ProblemEditorVoiceReviewSheet-Send'));

    await waitFor(() =>
      expect(screen.getByTestId('ProblemEditorAlternativePicker')).toBeTruthy()
    );
    expect(screen.getByTestId('ProblemEditorAlternativePicker-Transcript').textContent).toBe(
      '“return the two indices that add up to the target”'
    );

    await userEvent.click(screen.getByTestId('ProblemEditorAlternativePicker-Row-1'));

    await waitFor(() => {
      expect(editor.text()).toBe(`${starter}return [1, 2]`);
    });
    expect(editor.text()).not.toContain('return [0, 1]');
    await waitFor(() => expect(editor.diffRanges()).toEqual(['return [1, 2]']));
    expect(screen.getByTestId('ProblemEditorDiffToggle')).toBeTruthy();
  }, 60_000); // Full record→review→pick flow on the real screen: ~6 s alone, over 20 s under the parallel pre-commit run.

  it('given the starter with its indented empty body, when the planner inserts an already indented block after that line, then the block sits one level into the method and nothing doubles its indentation', async () => {
    const audio = createFakeAudioRecorder();
    const voice = createFakeVoiceEditTransport({
      transcript: 'declare an empty dictionary called seen and return an empty list',
      alternatives: [
        {
          id: 'alt-1',
          label: 'Declare seen',
          summary: 'Inserts seen and a return after the signature.',
          confidence: 1,
          commands: [{ type: 'insertAfterLine', line: 3, text: '        seen = {}\n        return []' }],
        },
      ],
    }) as unknown as VoiceEditTransportPort;
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice, audio });
    const editor = await session.editor();
    const starter = editor.text();
    expect(starter.endsWith(':\n        ')).toBe(true);

    await userEvent.click(screen.getByTestId('ProblemEditorVoiceStartButton'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorVoiceToolbar-Send')).toBeTruthy());
    await userEvent.click(screen.getByTestId('ProblemEditorVoiceToolbar-Send'));
    await waitFor(() =>
      expect(screen.getByTestId('ProblemEditorVoiceReviewSheet-Send')).toBeTruthy()
    );
    await userEvent.click(screen.getByTestId('ProblemEditorVoiceReviewSheet-Send'));

    await waitFor(() => {
      expect(editor.text()).toBe(`${starter}seen = {}\n        return []`);
    });
  }, 60_000);

  it('given the problem editor with a failing voice transport, when the user records and sends, then the compact error is shown', async () => {
    const audio = createFakeAudioRecorder();
    const voice = createFakeVoiceEditTransport({
      transcript: '',
      alternatives: [],
      failWith: new Error('planner exploded while planning'),
    }) as unknown as VoiceEditTransportPort;
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice, audio });

    await session.editor();

    await userEvent.click(screen.getByTestId('ProblemEditorVoiceStartButton'));

    await waitFor(() => expect(screen.getByTestId('ProblemEditorVoiceToolbar')).toBeTruthy());

    await userEvent.click(screen.getByTestId('ProblemEditorVoiceToolbar-Send'));

    await waitFor(() =>
      expect(screen.getByTestId('ProblemEditorVoiceErrorBanner')).toBeTruthy()
    );
    expect(screen.getByTestId('ProblemEditorVoiceErrorText').textContent).toBe(
      'planner exploded while planning'
    );
  });
});