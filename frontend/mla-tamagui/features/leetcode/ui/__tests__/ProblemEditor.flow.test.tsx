
import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  fireEvent,
  routerHarness,
  setStackHeadersRendered,
  screen,
  userEvent,
  waitFor,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../domain/problemCatalog';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';

// Native Expo/RN modules the screen's module graph pulls in but that cannot
// load under jsdom. They are never exercised here -- the ports (audio, voice)
// are injected, the language wheel's scroll is driven by its rows, and the
// toasts/clipboard are side effects -- so only their module-level imports
// need to resolve. expo-audio is stubbed by the harness (flowTestAliases).


// The wheel picker's scroll-to-select is real-DOM mechanics jsdom cannot run;
// its rows tap straight into onValueChanged, which is the screen-facing seam.

import {
  type LeetCodeGatewayPort,
  type LeetCodePortOverrides,
  type RunResponse,
  type VoiceEditTransportPort,
} from '../..';

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

afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

async function renderEditor(
  gateway: LeetCodeGatewayPort,
  search?: Record<string, string>
): Promise<FlowSession> {
  useRunnerSettingStore.setState({ runner: 'backend' });
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  return app.render('/leetcode/two-sum', { overrides: { leetcode: ports }, search });
}

async function formatThroughMenu(): Promise<void> {
  await userEvent.click(screen.getByTestId('EditorSettingsButton'));
  await waitFor(() => expect(screen.getByTestId('EditorSettingsButtonContent')).toBeTruthy());
  await userEvent.click(screen.getByTestId('EditorSettingsFormat'));
  await userEvent.click(screen.getByTestId('EditorSettingsFormatConfirm'));
}

describe('ProblemEditorScreen flow (editor buffer)', () => {
  // The export's prerendered editor frame posts its only 'ready' before hydration; 297214d fixed losing it.
  it('given the prerendered editor route, when the app hydrates, then the real CodeMirror shows the starter code', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);

    const editor = await session.editor();

    expect(editor.text()).toContain('def twoSum');
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
    await waitFor(async () => {
      expect((await editor.config()).language).toBe('javascript');
    });
  });

  async function switchTo(label: 'JavaScript' | 'Python') {
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await waitFor(() => {
      expect(screen.getByTestId(`ProblemEditorLanguagePicker-${label}`)).toBeTruthy();
    });
    await userEvent.click(screen.getByTestId(`ProblemEditorLanguagePicker-${label}`));
    await waitFor(() => {
      expect(screen.getByTestId('ProblemEditorLanguagePicker-Trigger').textContent).toBe(label);
    });
  }

  it('given JavaScript on screen, when the user switches back to Python, then the editor is on python with the Python starter', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    await switchTo('JavaScript');
    await waitFor(async () => {
      expect((await editor.config()).language).toBe('javascript');
    });

    await switchTo('Python');

    await waitFor(() => {
      expect(editor.text()).toContain('def twoSum');
    });
    expect((await editor.config()).language).toBe('python');
  });

  it('given JavaScript on screen and the caret after an opening brace, when Enter is pressed, then the next line is indented (the page is on the JavaScript grammar)', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    await switchTo('JavaScript');
    await waitFor(async () => {
      expect((await editor.config()).language).toBe('javascript');
    });
    await editor.setText('function f() {');
    await editor.select(14);

    await editor.press('Enter');

    await waitFor(() => {
      expect(editor.text()).toBe('function f() {\n    ');
    });
  });

  it('given the language switched as soon as the screen renders, whenever the editor hydrates, then the editor ends on the new language', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    await switchTo('JavaScript');

    const editor = await session.editor();

    await waitFor(async () => {
      expect((await editor.config()).language).toBe('javascript');
    });
    await waitFor(() => {
      expect(editor.text()).toContain('twoSum(nums, target)');
    });
  });
});

describe('ProblemEditorScreen flow (the editor language and its own keys)', () => {
  it('given the real editor on two-sum, when its config is read, then the language is python', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();

    expect((await editor.config()).language).toBe('python');
  });

  it('given Python text ending in a colon with the caret at its end, when Enter is pressed, then the next line is indented one level', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    await editor.setText('if x:');
    await editor.select(5);

    await editor.press('Enter');

    await waitFor(() => {
      expect(editor.text()).toBe('if x:\n    ');
    });
  });

  it('given Python text ending in an opening brace, when Enter is pressed, then the next line is not indented', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    await editor.setText('x = 1 {');
    await editor.select(7);

    await editor.press('Enter');

    await waitFor(() => {
      expect(editor.text()).toBe('x = 1 {\n');
    });
  });
});

describe('ProblemEditorScreen flow (languages a problem offers)', () => {
  // the route's problem comes from the real catalog, and every catalog problem has all six languages, so one is cut down to its Python starter for the test
  async function renderMeetingRooms(): Promise<FlowSession> {
    useRunnerSettingStore.setState({ runner: 'backend' });
    const ports: LeetCodePortOverrides = {
      gateway: fakeGateway().gateway,
      voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
      audio: createFakeAudioRecorder(),
    };
    return app.render('/leetcode/meeting-rooms', { overrides: { leetcode: ports } });
  }

  it('given a problem with only a Python starter, when the editor opens, then the language trigger is disabled and says Python', async () => {
    // Given the editor of a problem that has only a Python starter
    const problem = ProblemCatalog.getByIdOrThrow('meeting-rooms');
    const allStarters = problem.starterCode;
    problem.starterCode = { python3: allStarters.python3 ?? '' };
    try {
      const session = await renderMeetingRooms();
      const editor = await session.editor();

      // Then the starter is Python and there is no other language to switch to
      expect(editor.text()).toContain('def ');
      const trigger = screen.getByTestId('ProblemEditorLanguagePicker-Trigger');
      expect(trigger.textContent).toBe('Python');
      expect((trigger as HTMLButtonElement).disabled || trigger.getAttribute('aria-disabled') === 'true').toBe(true);
    } finally {
      problem.starterCode = allStarters;
    }
  });

  it('given a problem with starters in two languages, when the language sheet opens, then both are offered', async () => {
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    await session.editor();

    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));

    await waitFor(() => expect(screen.getByTestId('ProblemEditorLanguagePicker-Python')).toBeTruthy());
    expect(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript')).toBeTruthy();
  });
});

describe('ProblemEditorScreen flow (action buttons)', () => {
  it('given the problem editor, when the user taps format, then the gateway formats and the editor buffer is replaced', async () => {
    setStackHeadersRendered(true);
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();

    await formatThroughMenu();

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
    setStackHeadersRendered(true);
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

describe('ProblemEditorScreen flow (writes from outside the editor)', () => {
  it('given the code formatted by the gateway, when the user taps Undo, then the code before the format returns in one step', async () => {
    // Given the hydrated editor and the code it shows
    setStackHeadersRendered(true);
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    const before = editor.text();

    // When the gateway's format replaces the buffer, and the user taps the toolbar's Undo
    await formatThroughMenu();
    await waitFor(() => expect(editor.text()).toBe(`# formatted\n${before}`), { timeout: 5_000 });
    Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
    window.dispatchEvent(new Event('mcp:keyboardframe'));
    await screen.findByTestId('EditorAction-Undo', {}, { timeout: 5_000 });
    await session.tap('EditorAction-Undo');

    // Then the format is one undo step and the code is what it was
    await waitFor(() => expect(editor.text()).toBe(before), { timeout: 5_000 });
  });
});

describe('ProblemEditorScreen flow (keyboard)', () => {
  it('given the editor holds the caret, when the user taps the page outside the editor, then the editor lets go of it and the screen logs the dismissal', async () => {
    // Given the hydrated editor with the caret in it
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    await editor.focus();
    expect(editor.focused()).toBe(true);
    await forgetLoggedLines();

    // When the user taps the row under the editor, which is part of the page and not of the editor
    await session.tap('ProblemEditorDiffRow');

    // Then the editor no longer holds the caret, and the dismissal is in the log
    await waitFor(() => expect(editor.focused()).toBe(false));
    expect(await loggedLines('editor_v2.keyboard_dismiss')).not.toHaveLength(0);
  });

  it('given the editor holds the caret, when a tap lands on the editor zone itself, then the keyboard stays up', async () => {
    // Given the hydrated editor with the caret in it
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    await editor.focus();
    await forgetLoggedLines();

    // When a tap lands on the editor's own area (a phone reports it to the page as well as to the WebView). A click event, not a pointer press: a press on a
    // box that cannot take focus blurs the focused frame in jsdom, which a phone's tap on the WebView does not.
    await act(async () => {
      fireEvent.click(screen.getByTestId('ProblemEditorEditorZone'));
    });

    // Then the page does not take it for a tap away: nothing is dismissed and the editor still holds the caret
    expect(await loggedLines('editor_v2.keyboard_dismiss')).toHaveLength(0);
    expect(editor.focused()).toBe(true);
  });

  it('given the editor holds the caret, when the user taps Submit, then the screen dismisses the keyboard before it runs', async () => {
    // Given the hydrated editor with the caret in it
    const { gateway } = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    await editor.focus();
    await forgetLoggedLines();

    // When the user taps Submit
    await session.tap('ConsoleBarSubmit');

    // Then the dismissal is logged, so the result does not land under the keyboard, and the editor has let go of the caret
    await waitFor(() => expect(editor.focused()).toBe(false));
    expect(await loggedLines('editor_v2.keyboard_dismiss')).not.toHaveLength(0);
  });
});
