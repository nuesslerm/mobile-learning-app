import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { useCodeHistoryStore } from '../../state/codeHistoryStore';
import { useResetCodeRequestStore } from '../../state/resetCodeRequestStore';
import {
  type LeetCodeGatewayPort,
  type LeetCodePortOverrides,
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

async function renderVoiceEditor(ports: LeetCodePortOverrides) {
  return app.render('/leetcode/two-sum', { overrides: { leetcode: { ...ports, flags: { aiEdit: true } } } });
}

const STALE_BANNER = 'The code changed while the edit was being planned. Say it again.';

const SEEN_EDIT = {
  id: 'alt-1',
  label: 'Declare seen',
  summary: 'Inserts seen after the signature.',
  confidence: 1,
  commands: [{ type: 'insertAfterLine', line: 3, text: '        seen = {}' }],
};

// A plan that is in flight while the person goes on typing: the planner's answer waits until the test lets it go, and every request it
// was asked is kept.
function heldPlan(alternatives: (typeof SEEN_EDIT)[]) {
  const base = createFakeVoiceEditTransport({ transcript: 'declare an empty dictionary called seen', alternatives });
  const asked: string[] = [];
  let release: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const voice = {
    ...base,
    plan: async (request: { currentCode: string; transcript: string; language: string }) => {
      asked.push(request.currentCode);
      await gate;
      return base.plan(request);
    },
  } as unknown as VoiceEditTransportPort;
  return { voice, asked, release };
}

// A typed instruction, through the review sheet, as far as Send.
async function sendTypedInstruction(text: string) {
  await userEvent.click(screen.getByTestId('ProblemEditorTextEditStartButton'));
  await userEvent.type(await screen.findByTestId('ProblemEditorVoiceReviewSheet-TranscriptInput'), text);
  await userEvent.click(screen.getByTestId('ProblemEditorVoiceReviewSheet-Send'));
}

const voiceEditSnapshots = () => useCodeHistoryStore.getState().snapshots.filter((snap) => snap.reason === 'voice-edit');

describe('ProblemEditorScreen flow (voice edit)', () => {
  beforeEach(() => useCodeHistoryStore.setState({ snapshots: [] }));

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
    // the field shows once the editor has been read for the plan and the sheet is reviewing, not while it still transcribes
    const transcriptInput = await screen.findByTestId('ProblemEditorVoiceReviewSheet-TranscriptInput');
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
    expect(await editor.marks()).toEqual([{ id: 'edit', kind: 'edit', range: { from: starter.length, to: starter.length + 'return [1, 2]'.length } }]);
    expect(await screen.findByTestId('ProblemEditorDiffToggle')).toBeTruthy();
    // the edit comes with a selection, and the editor takes focus with it (the keyboard and toolbar are back for the next edit)
    await waitFor(() => expect(editor.focused()).toBe(true));
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
      expect(screen.getByTestId('ProblemEditorVoiceReviewSheet-TranscriptInput')).toBeTruthy()
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

  it("given a keystroke typed in the editor, when Send is pressed, then the planner is asked with the editor's text including it", async () => {
    // Given a keystroke in the editor and a plan that is held once asked
    const held = heldPlan([SEEN_EDIT]);
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice: held.voice, audio: createFakeAudioRecorder() });
    const editor = await session.editor();
    await editor.type('# ');
    const typed = editor.text();

    // When the instruction is sent
    await sendTypedInstruction('declare an empty dictionary called seen');

    // Then the planner saw the editor's own text
    await waitFor(() => expect(held.asked).toEqual([typed]));
  }, 60_000);

  it('given a plan held in flight, when the person types in the editor and the plan then returns, then the edit is refused with the banner text, the typed text is in the buffer, the plan text is not, and history has no voice-edit snapshot', async () => {
    // Given an instruction sent and its plan not yet returned
    const held = heldPlan([SEEN_EDIT]);
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice: held.voice, audio: createFakeAudioRecorder() });
    const editor = await session.editor();
    await sendTypedInstruction('declare an empty dictionary called seen');
    await waitFor(() => expect(held.asked).toHaveLength(1));

    // When the person types before the plan returns
    await editor.type('# ');
    const typed = editor.text();
    held.release();

    // Then the edit is refused with the banner text and the buffer is what the person typed
    await waitFor(() => expect(screen.getByTestId('ProblemEditorVoiceErrorText').textContent).toBe(STALE_BANNER));
    expect(editor.text()).toBe(typed);
    expect(editor.text()).not.toContain('seen = {}');
    expect(editor.diffRanges()).toEqual([]);
    expect(await editor.marks()).toEqual([]);
    expect(screen.queryByTestId('ProblemEditorDiffToggle')).toBeNull();
    expect(voiceEditSnapshots()).toEqual([]);
  }, 60_000);

  it('given an applied voice edit, when Undo is pressed once, then the text is exactly the text before the edit', async () => {
    // Given a voice edit that landed
    const voice = createFakeVoiceEditTransport({ transcript: 'declare seen', alternatives: [SEEN_EDIT] }) as unknown as VoiceEditTransportPort;
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice, audio: createFakeAudioRecorder() });
    const editor = await session.editor();
    const starter = editor.text();
    await sendTypedInstruction('declare seen');
    await waitFor(() => expect(editor.text()).toBe(`${starter}seen = {}`));
    Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
    window.dispatchEvent(new Event('mcp:keyboardframe'));
    await waitFor(() => expect(screen.queryByTestId('EditorAction-Undo')).not.toBeNull());

    // When Undo is pressed once
    await session.tap('EditorAction-Undo');

    // Then the whole edit is gone in that one step
    await waitFor(() => expect(editor.text()).toBe(starter));
  }, 60_000);

  it('given the picker open and the editor changed meanwhile, when a row is picked, then the pick is refused and the buffer is untouched', async () => {
    // Given two alternatives to choose from
    const alternatives = [
      { ...SEEN_EDIT, id: 'alt-1', confidence: 0.6, commands: [{ type: 'insertAfterLine', line: 3, text: '        return [0, 1]' }] },
      { ...SEEN_EDIT, id: 'alt-2', confidence: 0.5, commands: [{ type: 'insertAfterLine', line: 3, text: '        return [1, 2]' }] },
    ];
    const voice = createFakeVoiceEditTransport({ transcript: 'return the pair', alternatives }) as unknown as VoiceEditTransportPort;
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice, audio: createFakeAudioRecorder() });
    const editor = await session.editor();
    await sendTypedInstruction('return the pair');
    await screen.findByTestId('ProblemEditorAlternativePicker');

    // When the editor changes while the picker is open and a row is picked
    await editor.type('# ');
    const typed = editor.text();
    await userEvent.click(screen.getByTestId('ProblemEditorAlternativePicker-Row-1'));

    // Then the pick is refused, nothing of it is in the buffer and nothing was recorded
    await waitFor(() => expect(screen.getByTestId('ProblemEditorVoiceErrorText').textContent).toBe(STALE_BANNER));
    expect(editor.text()).toBe(typed);
    expect(await editor.marks()).toEqual([]);
    expect(voiceEditSnapshots()).toEqual([]);
  }, 60_000);

  it('given an applied voice edit, when the toggle is turned off, the person types before the edit and the toggle is turned on, then the highlight is the same text at its moved place', async () => {
    // Given a voice edit that landed and is highlighted
    const voice = createFakeVoiceEditTransport({ transcript: 'declare seen', alternatives: [SEEN_EDIT] }) as unknown as VoiceEditTransportPort;
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice, audio: createFakeAudioRecorder() });
    const editor = await session.editor();
    const starter = editor.text();
    await sendTypedInstruction('declare seen');
    await waitFor(() => expect(editor.diffRanges()).toEqual(['seen = {}']));

    // When the toggle is turned off, nothing is drawn and the mark is kept
    await userEvent.click(await screen.findByTestId('ProblemEditorDiffToggle'));
    await waitFor(() => expect(editor.diffRanges()).toEqual([]));
    expect(await editor.marks()).toMatchObject([{ id: 'edit', kind: 'edit', hidden: true }]);

    // and the person types before the edit and turns it on again
    await editor.select(0);
    await editor.type('# ');
    await userEvent.click(screen.getByTestId('ProblemEditorDiffToggle'));

    // Then the same text is highlighted where the typing moved it
    await waitFor(() => expect(editor.diffRanges()).toEqual(['seen = {}']));
    expect(await editor.marks()).toEqual([{ id: 'edit', kind: 'edit', range: { from: starter.length + 2, to: starter.length + 2 + 'seen = {}'.length } }]);
  }, 60_000);

  it('given an applied voice edit, when the code is reset, then no edit mark is drawn or held and the toggle is gone', async () => {
    // Given a voice edit that landed and is highlighted
    const voice = createFakeVoiceEditTransport({ transcript: 'declare seen', alternatives: [SEEN_EDIT] }) as unknown as VoiceEditTransportPort;
    const session = await renderVoiceEditor({ gateway: fakeGateway(), voice, audio: createFakeAudioRecorder() });
    const editor = await session.editor();
    const starter = editor.text();
    await sendTypedInstruction('declare seen');
    await waitFor(() => expect(editor.diffRanges()).toEqual(['seen = {}']));

    // When the person resets the code and confirms (the flow router does not mount the reset route's dialog, whose Reset button calls the request's onConfirm)
    await screen.findByTestId('ProblemEditorDiffToggle');
    await userEvent.click(screen.getByTestId('ProblemEditorResetButton'));
    await act(async () => useResetCodeRequestStore.getState().payload?.onConfirm());

    // Then the starter is back, nothing is highlighted and the toggle is gone
    await waitFor(() => expect(editor.text()).toBe(starter));
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.diffRanges()).toEqual([]);
    await waitFor(() => expect(screen.queryByTestId('ProblemEditorDiffToggle')).toBeNull());
  }, 60_000);
});