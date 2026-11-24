import { act, createFakeAudioRecorder, createFakeVoiceEditTransport, fireEvent, routerHarness, screen, setStackHeadersRendered, userEvent, waitFor, within, type FlowSession } from '@lib/flow-test-harness';
import { BackHandler, Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, VoiceEditTransportPort } from '../../..';
import { jsonText } from '../../../domain/console/caseFields';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { useConsoleCasesStore } from '../../../state/console/consoleCasesStore';
import { createFakeCodeRunner } from '../../../testing';

const gateway = {
  userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
  test: vi.fn(),
  submit: vi.fn(),
  poll: vi.fn(),
  runLocal: vi.fn(),
  format: vi.fn(async (p: { lang: string; code: string }) => p.code),
  loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
} as unknown as LeetCodeGatewayPort;

const parentheses = ProblemCatalog.getByIdOrThrow('valid-parentheses');
const twoSum = ProblemCatalog.getByIdOrThrow('two-sum');

async function openEditor(route = '/leetcode/valid-parentheses', flags?: { aiEdit: boolean }): Promise<FlowSession> {
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    runner: createFakeCodeRunner(),
    ...(flags ? { flags } : {}),
  };
  const session = await app.render(route, { overrides: { leetcode: ports } });
  await session.editor();
  return session;
}

async function setKeyboard(visible: boolean): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: visible });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  if (visible) await waitFor(() => expect(screen.queryByTestId('EditorHelperRails')).not.toBeNull(), { timeout: 5_000 });
  else await waitFor(() => expect(screen.queryByTestId('EditorHelperRails')).toBeNull(), { timeout: 5_000 });
}

const tap = (id: string) => userEvent.click(screen.getByTestId(id));
const text = (id: string) => screen.getByTestId(id).textContent;
const selected = (id: string) => screen.getByTestId(id).getAttribute('aria-selected');
const chipCount = () => screen.getAllByTestId(/^ConsoleChip-\d+$/).length;

function resetStores() {
  for (const problem of [parentheses, twoSum]) useConsoleCasesStore.getState().reset(problem);
}

beforeEach(() => {
  resetStores();
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: false });
});

describe('the console sheet and its tabs', () => {
  it('given the keyboard down, when the editor opens and Console is tapped, then the sheet opens on Testcase and covers the dock, and the Run Result tab shows its empty state', async () => {
    await openEditor();
    expect(screen.getByTestId('ConsoleBarOpen')).toBeTruthy();
    await forgetLoggedLines();

    await tap('ConsoleBarOpen');

    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    expect(selected('ConsoleTabs-testcase')).toBe('true');
    expect(screen.getByTestId('ConsoleDock').getAttribute('aria-hidden')).toBe('true');
    expect(await loggedLines('editor_v2.console.open')).not.toHaveLength(0);

    await tap('ConsoleTabs-result');

    expect(text('ConsoleResultEmpty')).toBe('Run your code to see the result here');
    expect(selected('ConsoleTabs-result')).toBe('true');
    expect(selected('ConsoleTabs-testcase')).toBe('false');
  });
});

describe('the Testcase tab', () => {
  it('given valid-parentheses, when the Testcase tab shows and a chip is tapped, then the chips follow the catalog and the field shows that case as JSON', async () => {
    await openEditor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());

    expect(chipCount()).toBe(parentheses.testCases?.length);
    expect(selected('ConsoleChip-1')).toBe('true');

    await tap('ConsoleChip-3');

    expect(selected('ConsoleChip-3')).toBe('true');
    expect(text('ConsoleField-s')).toBe(jsonText(parentheses.testCases?.[2]?.inputs['s']));
    expect(text('ConsoleField-s-Caption')).toBe('s');
  });

  it('given two-sum, when the Testcase tab shows, then an array parameter reads as JSON', async () => {
    await openEditor('/leetcode/two-sum');
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    const full = jsonText(twoSum.testCases?.[0]?.inputs['nums']);

    expect(text('ConsoleField-nums')).toBe(full);
  });

  it('given Case 3 selected, when + is tapped, then a custom case seeded with its values is selected and editable, and typing keeps the text', async () => {
    await openEditor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    const bundled = chipCount();
    await tap('ConsoleChip-3');

    await tap('ConsoleChipAdd');

    expect(chipCount()).toBe(bundled + 1);
    expect(selected(`ConsoleChip-${bundled + 1}`)).toBe('true');
    const field = screen.getByTestId('ConsoleField-s') as HTMLTextAreaElement;
    expect(field.value).toBe(jsonText(parentheses.testCases?.[2]?.inputs['s']));
    fireEvent.change(field, { target: { value: '"(("' } });
    expect((screen.getByTestId('ConsoleField-s') as HTMLTextAreaElement).value).toBe('"(("');
    expect(useConsoleCasesStore.getState().drafts[parentheses.id]?.items[0]?.fields['s']).toBe('"(("');
  });

  it('given a bundled case, when the pencil is tapped, then an editable copy is selected and the bundled case is unchanged', async () => {
    await openEditor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    const bundled = chipCount();

    await tap('ConsoleChipEdit');

    expect(chipCount()).toBe(bundled + 1);
    expect(screen.getByTestId('ConsoleField-s').tagName).not.toBe('DIV');
    await tap('ConsoleChip-1');
    expect(text('ConsoleField-s')).toBe(jsonText(parentheses.testCases?.[0]?.inputs['s']));
  });

  it('given a custom case, when Delete case is tapped, then it is removed and the previous chip is selected', async () => {
    await openEditor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    const bundled = chipCount();
    await tap('ConsoleChipAdd');

    await tap('ConsoleDeleteCase');

    expect(chipCount()).toBe(bundled);
    expect(selected(`ConsoleChip-${bundled}`)).toBe('true');
    expect(screen.queryByTestId('ConsoleDeleteCase')).toBeNull();
  });

  it('given two custom cases, when the footer Reset is tapped, then only the bundled chips remain and Case 1 is selected', async () => {
    await openEditor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    const bundled = chipCount();
    await tap('ConsoleChipAdd');
    await tap('ConsoleChipAdd');
    expect(chipCount()).toBe(bundled + 2);

    await tap('ConsoleFooterReset');

    expect(chipCount()).toBe(bundled);
    expect(selected('ConsoleChip-1')).toBe('true');
  });

  it('given ten custom cases, when + is looked at, then it is disabled', async () => {
    await openEditor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    act(() => {
      for (let i = 0; i < 10; i += 1) useConsoleCasesStore.getState().addCase(parentheses);
    });

    await waitFor(() => expect(screen.getByTestId('ConsoleChipAdd').getAttribute('aria-disabled')).toBe('true'));
  });
});

describe('the action bar above the keyboard', () => {
  it('given the keyboard up, when the editor shows, then the rail carries the bar and the dock is not mounted', async () => {
    await openEditor();
    await setKeyboard(true);

    expect(screen.getByTestId('ConsoleActionBar')).toBeTruthy();
    expect(screen.queryByTestId('ConsoleDock')).toBeNull();
    expect(screen.getByTestId('EditorHelperRails').contains(screen.getByTestId('ConsoleBarOpen'))).toBe(true);
  });

  it('given the keyboard up, when Console is tapped, then the keyboard is dismissed, the rail is gone and the sheet is open', async () => {
    await openEditor();
    await setKeyboard(true);
    await forgetLoggedLines();

    await tap('ConsoleBarOpen');

    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    expect(await loggedLines('editor_v2.keyboard_dismiss')).not.toHaveLength(0);
    expect(screen.queryByTestId('ConsoleActionBar')).toBeNull();
  });

  it('given the rail with two custom cases, when Reset is tapped, then the cases reset and the rail stays', async () => {
    await openEditor();
    act(() => {
      useConsoleCasesStore.getState().addCase(parentheses);
      useConsoleCasesStore.getState().addCase(parentheses);
    });
    await setKeyboard(true);

    await tap('ConsoleBarReset');

    expect(useConsoleCasesStore.getState().drafts[parentheses.id]?.items).toHaveLength(0);
    expect(screen.getByTestId('ConsoleActionBar')).toBeTruthy();
  });
});

describe('the close paths', () => {
  it('given the console open, when its handle is pressed, then it closes and the dock returns', async () => {
    await openEditor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    await forgetLoggedLines();

    await tap('ConsoleSheetHandle');

    await waitFor(() => expect(screen.queryByTestId('ConsoleSheet')).toBeNull());
    expect(await loggedLines('editor_v2.console.close')).not.toHaveLength(0);
    expect(screen.getByTestId('ConsoleDock')).toBeTruthy();
  });

  it('given the console open, when the editor takes focus, then it closes with that reason', async () => {
    const session = await openEditor();
    const editor = await session.editor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    await forgetLoggedLines();

    await editor.focus();

    await waitFor(() => expect(screen.queryByTestId('ConsoleSheet')).toBeNull());
    expect(JSON.stringify(await loggedLines('editor_v2.console.close'))).toContain('editor-focus');
  });

  describe('on Android', () => {
    const realOS = Platform.OS;
    afterEach(() => {
      (Platform as { OS: string }).OS = realOS;
      vi.restoreAllMocks();
    });

    it('given the console open, when the hardware back handler runs, then it closes and answers true, and while closed no handler is registered', async () => {
      (Platform as { OS: string }).OS = 'android';
      const handlers: Parameters<typeof BackHandler.addEventListener>[1][] = [];
      // oxlint-disable-next-line no-restricted-properties -- react-native's BackHandler is a native API with no runtime in jsdom
      const add = vi.spyOn(BackHandler, 'addEventListener').mockImplementation((_event, handler) => {
        handlers.push(handler);
        return { remove: () => void handlers.splice(handlers.indexOf(handler), 1) };
      });
      await openEditor();
      expect(handlers).toHaveLength(0);

      await tap('ConsoleBarOpen');
      await waitFor(() => expect(handlers).toHaveLength(1));
      let answer = false;
      await waitFor(() => {
        answer = handlers[0]?.({} as never) ?? false;
      });

      expect(answer).toBe(true);
      await waitFor(() => expect(screen.queryByTestId('ConsoleSheet')).toBeNull());
      expect(handlers).toHaveLength(0);
      expect(add).toHaveBeenCalled();
    });
  });
});

describe('the Console overlay', () => {
  it('given the editor is laid out, when Console opens and closes, then the editor keeps its layout and iframe', async () => {
    const session = await openEditor();
    await session.layout('ProblemEditorScreen', { x: 0, y: 0, width: 400, height: 700 });
    const content = () => screen.getByTestId('ProblemEditorScroll').firstElementChild as HTMLElement;
    const editorZone = screen.getByTestId('ProblemEditorEditorZone');
    const editorStyle = editorZone.getAttribute('style');
    const editorClass = editorZone.getAttribute('class');
    const iframe = editorZone.querySelector('iframe');
    expect(iframe).not.toBeNull();
    expect(content().style.paddingBottom).toBe('24px');

    await tap('ConsoleBarOpen');

    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    expect(content().style.paddingBottom).toBe('24px');
    expect(editorZone.getAttribute('style')).toBe(editorStyle);
    expect(editorZone.getAttribute('class')).toBe(editorClass);
    expect(editorZone.querySelector('iframe')).toBe(iframe);
    expect(screen.getByTestId('ConsoleDock').getAttribute('aria-hidden')).toBe('true');

    await tap('ConsoleSheetHandle');

    await waitFor(() => expect(screen.queryByTestId('ConsoleSheet')).toBeNull());
    expect(editorZone.querySelector('iframe')).toBe(iframe);
    expect(screen.getByTestId('ConsoleDock').getAttribute('aria-hidden')).toBe('false');
  });
});

describe('the workspace', () => {
  it('given the console open, when the Plan tab is tapped, then the sheet is gone with that reason and stays closed on Code', async () => {
    await openEditor('/leetcode/valid-parentheses', { aiEdit: true });
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(screen.getByTestId('ConsoleSheet')).toBeTruthy());
    await forgetLoggedLines();

    await tap('ProblemWorkspaceTab-plan');

    await waitFor(() => expect(screen.queryByTestId('ConsoleSheet')).toBeNull());
    expect(JSON.stringify(await loggedLines('editor_v2.console.close'))).toContain('plan-tab');
    await tap('ProblemWorkspaceTab-code');
    expect(screen.queryByTestId('ConsoleSheet')).toBeNull();
  });
});

describe('the screen with the console on', () => {
  it('given the console on, when two-sum opens, then the pencil and the mic sit in the language row, and the header menu resets the code through the confirm route', async () => {
    const r = routerHarness();
    setStackHeadersRendered(true);
    await openEditor('/leetcode/two-sum', { aiEdit: true });

        const row = screen.getByTestId('ProblemEditorDiffRow');
    expect(within(row).getByTestId('ProblemEditorTextEditStartButton')).toBeTruthy();
    expect(within(row).getByTestId('ProblemEditorVoiceStartButton')).toBeTruthy();

    await userEvent.click(screen.getByTestId('ProblemEditorMenuButton'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorMenuButtonContent')).toBeTruthy());
    await userEvent.click(within(screen.getByTestId('ProblemEditorMenuButtonContent')).getByTestId('ProblemEditorMenuResetCode'));

    await waitFor(() => expect(r.lastPush()).toBe('/leetcode/two-sum/reset-code'));
  });
});
