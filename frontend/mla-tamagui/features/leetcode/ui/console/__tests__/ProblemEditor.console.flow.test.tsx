import { act, createFakeAudioRecorder, createFakeVoiceEditTransport, expectWarning, routerHarness, screen, setStackHeadersRendered, userEvent, waitFor, within, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import type { CodeRunnerPort, LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../../..';
import { createLiveLeetCodeGateway } from '../../../data/webBridgeClient';
import { useRunnerSettingStore } from '../../../state/runnerSettingStore';
import { createFakeCodeRunner, failingRun, passingRun } from '../../../testing';
import { fakeGateway, resetConsoleTwin, tap, text, verdict } from './consoleTwinSupport';

// the runs here go through the gateway, so the setting is the server unless a test hands in its own runner
async function renderEditor(gateway: LeetCodeGatewayPort, runner?: CodeRunnerPort): Promise<FlowSession> {
  if (!runner) useRunnerSettingStore.setState({ runner: 'backend' });
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    ...(runner ? { runner } : {}),
  };
  return app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
}

beforeEach(() => resetConsoleTwin('two-sum'));
afterEach(() => setStackHeadersRendered(false));

describe('ProblemEditorScreen flow with the console (editor buffer)', () => {
  it('given a hydrated problem editor, when the user types in the editor and taps Submit, then the typed text is the code the gateway runs', async () => {
    const gateway = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();
    expect(editor.text()).toContain('def twoSum');

    const typed = `${editor.text()}\n        return [0, 1]`;
    await editor.select(0, editor.text().length);
    await editor.type(typed);
    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(gateway.runLocal).toHaveBeenCalledWith(expect.objectContaining({ code: typed, lang: 'python3', methodName: 'twoSum' })));
  });

  it('given the editor hydrated with the starter code, when the user selects the pass body and types return [], then the run request buffer contains return []', async () => {
    const gateway = fakeGateway();
    const session = await renderEditor(gateway);
    const editor = await session.editor();

    const bodyStart = editor.text().lastIndexOf('\n') + 1;
    await editor.select(bodyStart, editor.text().length);
    await editor.type('return []');
    await tap('ConsoleBarRun');

    await waitFor(() => expect(gateway.runLocal).toHaveBeenCalledWith(expect.objectContaining({ code: expect.stringContaining('return []') })));
  });

  it('given the problem editor, when the user taps Submit, then the gateway verdict is shown with its tally', async () => {
    const gateway = fakeGateway();
    const session = await renderEditor(gateway);
    await session.editor();

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(text('ConsoleResultCounts')).toBe('5 / 5 testcases passed');
  });

  it('given the leetcode backend sits behind a proxy that answers a run with a 502 page, when the user taps Run, then Run failed shows and the log names the endpoint and status, then the problem', async () => {
    expectWarning(/leetcode\.api\.fail/);
    expectWarning(/editor_v2\.run_local\.error/);
    const realFetch = globalThis.fetch;
    vi.stubGlobal('fetch', (input: RequestInfo | URL, init?: RequestInit) =>
      String(input).includes('/api/leetcode/run') ? Promise.resolve(new Response('<html>Bad Gateway</html>', { status: 502, headers: { 'Content-Type': 'text/html' } })) : realFetch(input, init)
    );
    try {
      const session = await renderEditor(createLiveLeetCodeGateway('http://leetcode-backend.test'));
      await session.editor();
      await forgetLoggedLines();

      await tap('ConsoleBarRun');

      await waitFor(() => expect(verdict()).toBe('Run failed'));
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

describe('ProblemEditorScreen flow with the console (reset)', () => {
  it('given the problem editor with code on screen, when the user taps Reset code in the header menu, then the app pushes the reset-code route', async () => {
    const r = routerHarness();
    setStackHeadersRendered(true);
    const session = await renderEditor(fakeGateway());
    const editor = await session.editor();
    const typed = `${editor.text()}\n        return [0, 1]`;
    await editor.select(0, editor.text().length);
    await editor.type(typed);

    await userEvent.click(screen.getByTestId('ProblemEditorMenuButton'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorMenuButtonContent')).toBeTruthy());
    await userEvent.click(within(screen.getByTestId('ProblemEditorMenuButtonContent')).getByTestId('ProblemEditorMenuResetCode'));

    await waitFor(() => expect(r.lastPush()).toBe('/leetcode/two-sum/reset-code'));
  });
});

describe('ProblemEditorScreen flow with the console (code runner port)', () => {
  it('given a runner whose last case fails, when the user taps Submit, then the console shows that runner\'s verdict and the gateway is not asked', async () => {
    const gateway = fakeGateway();
    const runner = createFakeCodeRunner(() => failingRun(3));
    const session = await renderEditor(gateway, runner);
    await session.editor();

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(verdict()).toBe('Wrong Answer'));
    expect(text('ConsoleResultCounts')).toBe('2 / 3 testcases passed');
    expect(runner.calls).toEqual([expect.objectContaining({ lang: 'python3', methodName: 'twoSum' })]);
    expect(gateway.runLocal).not.toHaveBeenCalled();
  });

  it('given a runner whose cases all pass, when the user taps Submit, then the console shows it accepted', async () => {
    const session = await renderEditor(fakeGateway(), createFakeCodeRunner(() => passingRun(4)));
    await session.editor();

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(text('ConsoleResultCounts')).toBe('4 / 4 testcases passed');
  });

  it('given a run that is still going, when the user edits the code and the run then finishes, then no verdict is shown over the new code', async () => {
    let finish: (run: RunResponse) => void = () => undefined;
    const runner = createFakeCodeRunner(() => new Promise<RunResponse>((resolve) => (finish = resolve)));
    const session = await renderEditor(fakeGateway(), runner);
    const editor = await session.editor();
    await forgetLoggedLines();
    await tap('ConsoleBarSubmit');
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    const edited = `${editor.text()}\n        return [0, 1]`;
    await forgetLoggedLines();
    await editor.select(0, editor.text().length);
    await editor.type(edited);
    await waitFor(() => expect(editor.text()).toBe(edited));
    await waitFor(async () => expect(await loggedLines('editor.value_sync')).not.toHaveLength(0));
    await act(async () => finish(passingRun(3)));

    await waitFor(async () => expect(await loggedLines('editor_v2.run_local.discarded')).not.toHaveLength(0), { timeout: 10_000 });
    expect(screen.queryByTestId('ConsoleResultVerdict')).toBeNull();
  });
});
