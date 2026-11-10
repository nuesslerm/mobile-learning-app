import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  expectWarning,
  logAt,
  routerHarness,
  screen,
  userEvent,
  waitFor,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../flow/loggedLines';
import { describe, expect, it, vi } from 'vitest';

import { createLiveLeetCodeGateway } from '../../../../../features/leetcode/data/webBridgeClient';

// Native Expo/RN modules the screen's module graph pulls in but that cannot
// load under jsdom. They are never exercised here -- the ports (audio, voice)
// are injected, the language wheel's scroll is driven by its rows, and the
// toasts/clipboard are side effects -- so only their module-level imports
// need to resolve. expo-audio is stubbed by the harness (flowTestAliases).


// The wheel picker's scroll-to-select is real-DOM mechanics jsdom cannot run;
// its rows tap straight into onValueChanged, which is the screen-facing seam.

import {
  useLeetCodeAuthStore,
  type LeetCodeGatewayPort,
  type LeetCodePorts,
  type RunResponse,
  type VoiceEditTransportPort,
} from '../../../../../features/leetcode';

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

function fakeGateway(): { gateway: LeetCodeGatewayPort } {
  return {
    gateway: {
      userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
      test: vi.fn(async () => ({ ...ACCEPTED_RUN, interpret_id: 'flow-interpret' })),
      submit: vi.fn(async () => ACCEPTED_RUN),
      poll: vi.fn(async () => ACCEPTED_RUN),
      runLocal: vi.fn(async () => ACCEPTED_RUN),
      format: vi.fn(async (p: { lang: string; code: string }) => `# formatted\n${p.code}`),
      loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
    },
  };
}

async function renderEditor(gateway: LeetCodeGatewayPort, search?: Record<string, string>): Promise<FlowSession> {
  const ports: LeetCodePorts = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  return app.render('/leetcode/two-sum', { overrides: { leetcode: ports }, search });
}

describe('ProblemEditorScreen flow (editor buffer)', () => {
  // The export's prerendered editor frame posts its only 'ready' before hydration; 297214d fixed losing it.
  it('given the prerendered editor route, when the app hydrates, then the real CodeMirror shows the starter code', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);

    const editor = await session.editor();

    expect(editor.text()).toContain('def twoSum');
  });

  it('given a hydrated problem editor, when the user types in the editor, then the typed text is the code the gateway runs', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    expect(editor.text()).toContain('def twoSum');

    await session.snapshot('editor-hydrated');

    const typed = `${editor.text()}\n        return [0, 1]`;
    await editor.select(0, editor.text().length);
    await editor.type(typed);

    await userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));

    await waitFor(() => {
      expect(gateway.runLocal).toHaveBeenCalledWith(
        expect.objectContaining({ code: typed, lang: 'python3', methodName: 'twoSum' })
      );
    });
  });

  it('given the editor hydrated with the starter code, when the user selects the pass body and types return [], then the run request buffer contains return []', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();

    const bodyStart = editor.text().lastIndexOf('\n') + 1;
    await editor.select(bodyStart, editor.text().length);
    await editor.type('return []');

    await userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));

    await waitFor(() => {
      expect(gateway.runLocal).toHaveBeenCalledWith(
        expect.objectContaining({ code: expect.stringContaining('return []') })
      );
    });
  });

  it('given the problem editor, when the user runs the tests, then the gateway verdict is shown', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    await session.editor();

    await userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));

    await waitFor(() => {
      expect(screen.getByTestId('ProblemEditorRunOutcome')).toBeTruthy();
    });
    expect(screen.getByTestId('ProblemEditorRunOutcomeStatus').textContent).toBe('Accepted');
    expect(screen.getByTestId('ProblemEditorRunOutcomeTotals').textContent).toBe(
      '5 / 5 test cases passed'
    );

    await session.snapshot('run-accepted');
  });

  it('given the leetcode backend sits behind a proxy that answers a run with a 502 page, when the user runs the tests, then the error shows and the log names the endpoint and status, then the problem', async () => {
    // Given the real gateway over a fetch whose run endpoint answers 502
    expectWarning(/leetcode\.api\.fail/);
    expectWarning(/editor_v2\.run_local\.error/);
    const realFetch = globalThis.fetch;
    vi.stubGlobal('fetch', (input: RequestInfo | URL, init?: RequestInit) =>
      String(input).includes('/api/leetcode/run')
        ? Promise.resolve(new Response('<html>Bad Gateway</html>', { status: 502, headers: { 'Content-Type': 'text/html' } }))
        : realFetch(input, init)
    );
    try {
      const session = await renderEditor(createLiveLeetCodeGateway('http://leetcode-backend.test'));
      await session.editor();
      await forgetLoggedLines();

      // When the user runs the tests
      await userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));

      // Then the screen shows an error outcome
      expect((await screen.findByTestId('ProblemEditorRunOutcomeStatus')).textContent).toBe('Error');
      // And the adapter line has the endpoint, status and a duration, the screen line the problem, and neither holds the backend URL
      const [fail] = await loggedLines('leetcode.api.fail');
      expect(fail?.context).toMatchObject({ endpoint: 'run', method: 'POST', status: 502 });
      expect(typeof fail?.context?.ms).toBe('number');
      const [error] = await loggedLines('editor_v2.run_local.error');
      expect(error?.context).toMatchObject({ problemId: 'two-sum', lang: 'python3' });
      expect(JSON.stringify([fail, error])).not.toContain('leetcode-backend.test');
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe('ProblemEditorScreen flow (language toggle)', () => {
  it('given the problem editor, when the user switches the language to JavaScript, then the editor buffer and picker change', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    expect(editor.text()).toContain('def twoSum');

    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));

    await waitFor(() => {
      expect(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript')).toBeTruthy();
    });
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript'));

    await waitFor(() => {
      expect(screen.getByTestId('ProblemEditorLanguagePicker-Trigger').textContent).toBe(
        'JavaScript'
      );
    });
    await waitFor(() => {
      expect(editor.text()).toContain('twoSum(nums, target)');
    });
  });
});

describe('ProblemEditorScreen flow (action buttons)', () => {
  it('given a signed-in problem editor, when the user taps run, then the gateway is asked to test and poll the submission', async () => {
    useLeetCodeAuthStore.setState({ isLoggedIn: true, username: 'flow-user' });
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    await session.editor();

    await userEvent.click(screen.getByTestId('ProblemEditorRunButton'));

    await waitFor(() => {
      expect(gateway.test).toHaveBeenCalledWith(
        expect.objectContaining({
          slug: 'two-sum',
          lang: 'python3',
          dataInput: '[2,7,11,15]\n9',
        })
      );
      expect(gateway.poll).toHaveBeenCalledWith('flow-interpret');
    });
  });

  it('given the problem editor, when the user taps format, then the gateway formats and the editor buffer is replaced', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();

    await userEvent.click(screen.getByTestId('ProblemEditorFormatButton'));

    await waitFor(() => {
      expect(gateway.format).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(editor.text()).toContain('# formatted');
    });
  });
});

describe('ProblemEditorScreen flow (router)', () => {
  it('given a seed in the route params of a dev bundle, when the editor opens, then it shows that code instead of the starter', async () => {
    const { gateway } = fakeGateway();
    const seed = 'class Solution:\n    def twoSum(self, nums, target):\n        seen = {}';
    const session = await renderEditor(gateway, { seed });
    const editor = await session.editor();
    expect(editor.text()).toBe(seed);
  });

  it('given a base64url seed in the route params of a dev bundle, when the editor opens, then it shows that code with its newlines', async () => {
    const { gateway } = fakeGateway();
    const seed = 'class Solution:\n    def twoSum(self, nums, target):\n        seen = {}';
    const seed64 = Buffer.from(seed).toString('base64url');
    const session = await renderEditor(gateway, { seed64 });
    const editor = await session.editor();
    expect(editor.text()).toBe(seed);
  });

  it('given an open editor, when a link to the same problem with another seed arrives, then the editor shows that seed', async () => {
    // Given the editor opened from a seeded link (a test unit on a reused app)
    const r = routerHarness();
    const { gateway } = fakeGateway();
    const first = 'class Solution:\n    def twoSum(self, nums, target):\n        seen = {}';
    const second = `${first}\n        for i, n in enumerate(nums):`;
    const session = await renderEditor(gateway, { seed64: Buffer.from(first).toString('base64url') });
    expect((await session.editor()).text()).toBe(first);
    // When the next unit's link opens the same screen with the next seed (expo-router updates it, it does not remount)
    await act(async () => r.setParams({ id: 'two-sum', seed64: Buffer.from(second).toString('base64url') }));
    // Then the editor shows the new seed, not the code the last unit left behind
    await waitFor(async () => expect((await session.editor()).text()).toBe(second));
  });

  it('given the problem editor with code on screen, when the user taps reset, then the app pushes the reset-code route and the reset request line runs once', async () => {
    const r = routerHarness();
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    const typed = `${editor.text()}\n        return [0, 1]`;
    await editor.select(0, editor.text().length);
    await editor.type(typed);

    const probe = logAt('features/leetcode/ui/ProblemEditorScreen.tsx', 533, ['problem.id']);
    try {
      await userEvent.click(screen.getByTestId('ProblemEditorResetButton'));
    } finally {
      probe.release();
    }

    await waitFor(() => {
      expect(r.lastPush()).toBe('/leetcode/two-sum/reset-code');
    });
    expect(probe.stops).toHaveLength(1);
    expect(probe.stops[0].values).toEqual({ 'problem.id': 'two-sum' });
    expect(probe.stops[0].stack[0]).toMatchObject({ file: 'features/leetcode/ui/ProblemEditorScreen.tsx', line: 533 });
  });
});

describe('ProblemEditorScreen flow (syntax toolbar)', () => {
  // The soft keyboard up, signalled the way the device harness does: that raises the toolbar.
  async function raiseKeyboard(): Promise<void> {
    Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
    window.dispatchEvent(new Event('mcp:keyboardframe'));
    await screen.findByTestId('EditorSyntax-Paren', {}, { timeout: 5_000 });
  }

  it('given the caret at the end of the code, when the user taps Parentheses, then the editor adds the pair through its own bracket handling, caret between', async () => {
    // Given the hydrated editor with the caret after the last character
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    const before = editor.text();
    await editor.select(before.length);
    // And the soft keyboard up
    await raiseKeyboard();

    // When the user taps the toolbar's Parentheses key
    await session.tap('EditorSyntax-Paren');

    // Then "()" follows the code and the caret sits inside it, where typing the closer steps over it
    await waitFor(() => expect(editor.text()).toBe(`${before}()`));
    expect(editor.selection()).toEqual({ start: before.length + 1, end: before.length + 1 });
  });

  it('given "else" typed at the depth of an if block\'s body, when the user taps Colon, then the editor moves "else:" out to the if, as it does for a typed colon', async () => {
    // Given an if block whose body ends with "else", still at the body's depth, the caret after it
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    const code = 'class Solution:\n    def f(self, x):\n        if x:\n            return 1\n            else';
    await editor.setText(code);
    await editor.select(code.length);
    // And the soft keyboard up
    await raiseKeyboard();

    // When the user taps the toolbar's Colon key
    await session.tap('EditorSyntax-Colon');

    // Then the line reads "else:" at the depth of its if
    await waitFor(() => expect(editor.text().split('\n').at(-1)).toBe('        else:'));
  });
});
