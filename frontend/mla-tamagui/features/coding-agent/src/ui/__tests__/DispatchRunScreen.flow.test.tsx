import { createFixtureGateway } from '@feature/coding-agent/testing';
import { act, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { Linking } from 'react-native';
import { describe, expect, it, vi } from 'vitest';

import { app } from '../../__tests__/flowApp';

function renderRun(
  runId: string
): Promise<Awaited<ReturnType<typeof app.render>>> {
  return app.render(`/coding-agent/runs/${runId}`, {
    overrides: { 'coding-agent': { gateway: createFixtureGateway() } },
    search: { transport: 'fixture' },
  });
}

describe('DispatchRunScreen flow (fixture transport)', () => {
  it('given the finished fixture run, when the screen renders, then the header carries the model, tokens, and cost, and the feed carries the prompt, the first assistant text, and the three tool names', async () => {
    await renderRun('fx-run-finished');

    await waitFor(
      () => expect(screen.getByTestId('DispatchRunFeed')).toBeTruthy(),
      { timeout: 12_000 }
    );
    const header = screen.getByTestId('DispatchRunHeader');
    expect(header.textContent).toContain('minimax/MiniMax-M3');
    expect(header.textContent).toContain('12345');
    expect(header.textContent).toContain('$0.0123');

    const feed = screen.getByTestId('DispatchRunFeed');
    expect(feed.textContent).toContain(
      'Add the DispatchRunsSection to the thread list'
    );
    expect(feed.textContent).toContain('Adding the section now.');
    expect(feed.textContent).toContain('read');
    expect(feed.textContent).toContain('edit');
    expect(feed.textContent).toContain('bash');
  });

  it('given the fixture transport, when the screen renders, then there is no composer and no textbox role', async () => {
    await renderRun('fx-run-finished');
    await waitFor(
      () => expect(screen.getByTestId('DispatchRunFeed')).toBeTruthy(),
      { timeout: 12_000 }
    );

    expect(screen.queryByTestId('Composer')).toBeNull();
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('given the finished fixture run, when the dashboard button is clicked, then Linking.openURL is called with the run URL', async () => {
    // oxlint-disable-next-line no-restricted-properties -- spy on the React Native Linking module, not a project module
    const openURLSpy = vi
      .spyOn(Linking, 'openURL')
      .mockResolvedValue(undefined as never);
    try {
      await renderRun('fx-run-finished');
      await waitFor(
        () =>
          expect(screen.getByTestId('DispatchRunDashboardLink')).toBeTruthy(),
        { timeout: 12_000 }
      );

      await userEvent.click(screen.getByTestId('DispatchRunDashboardLink'));

      expect(openURLSpy).toHaveBeenCalledTimes(1);
      const calledUrl = openURLSpy.mock.calls[0]?.[0];
      expect(typeof calledUrl).toBe('string');
      expect(String(calledUrl).endsWith('/opencode-log/fx-run-finished')).toBe(
        true
      );
    } finally {
      openURLSpy.mockRestore();
    }
  });

  it('given a run id that does not exist, when the screen renders, then the error surface appears within 12s', async () => {
    await renderRun('does-not-exist');
    await waitFor(
      () => expect(screen.getByTestId('DispatchRunError')).toBeTruthy(),
      { timeout: 12_000 }
    );
  });
});

describe('DispatchRunScreen flow (fixture transport, running run)', () => {
  it('given the running fixture run, when the screen renders, then a working indicator is visible and the feed textContent length grows between two reads 2s apart', async () => {
    await renderRun('fx-run-running');

    await waitFor(
      () => expect(screen.getByTestId('WorkingIndicator')).toBeTruthy(),
      { timeout: 12_000 }
    );
    const feed = screen.getByTestId('DispatchRunFeed');
    const before = (feed.textContent ?? '').length;

    // the run keeps streaming while the test waits, so the wait is inside act
    await act(() => new Promise((resolve) => setTimeout(resolve, 2_000)));

    const after = (screen.getByTestId('DispatchRunFeed').textContent ?? '')
      .length;
    expect(after).toBeGreaterThan(before);
  });

  it('given the running fixture run, when the fixture gateway flips it to finished, then the header carries $0.0042 within 15s and the working indicator is gone', async () => {
    await renderRun('fx-run-running');

    await waitFor(
      () =>
        expect(screen.getByTestId('DispatchRunHeader').textContent).toContain(
          '$0.0042'
        ),
      { timeout: 15_000 }
    );

    expect(screen.queryByTestId('WorkingIndicator')).toBeNull();
  });
});
