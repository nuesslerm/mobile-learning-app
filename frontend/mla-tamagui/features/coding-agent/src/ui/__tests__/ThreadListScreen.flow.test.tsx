import {
  FIXTURE_IDS,
  FIXTURE_THREADS,
} from '../../domain';
import { createFixtureGateway } from '@feature/coding-agent/testing';
import {
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { app } from '../../__tests__/flowApp';

const FX_THREAD_PREFIX = 'fx-thread-';

function renderList() {
  return app.render('/coding-agent', {
    overrides: { 'coding-agent': { gateway: createFixtureGateway() } },
    search: { transport: 'fixture' },
  });
}

describe('ThreadListScreen flow (fixture transport)', () => {
  it('given fixture dispatch runs, when the list renders, then the Dispatch runs section shows both runs with agent and cost', async () => {
    await renderList();

    await waitFor(() => expect(screen.getByTestId('DispatchRunRow-fx-run-finished')).toBeTruthy());
    expect(screen.getByTestId('DispatchRunsSection')).toBeTruthy();
    expect(screen.getByTestId('DispatchRunRow-fx-run-running')).toBeTruthy();
    expect(screen.getByTestId('DispatchRunRow-fx-run-finished').textContent).toContain('implementer');
    expect(screen.getByTestId('DispatchRunRow-fx-run-finished').textContent).toContain('$0.0123');
  });

  it('given the finished run row, when tapped, then the app navigates to the read-only run screen with the transport preserved', async () => {
    const router = routerHarness();
    await renderList();
    await waitFor(() => expect(screen.getByTestId('DispatchRunRow-fx-run-finished')).toBeTruthy());

    await userEvent.click(screen.getByTestId('DispatchRunRow-fx-run-finished'));

    expect(router.lastPush()).toMatch(/^\/coding-agent\/runs\/fx-run-finished/);
  });
});

describe('ThreadListScreen flow (live threads, fixture transport)', () => {
  it('given the fixture transport, when the list renders, then every FIXTURE_IDS thread renders a row whose text contains its title', async () => {
    await renderList();

    const firstRowId = `ThreadRow-${FX_THREAD_PREFIX}${FIXTURE_IDS[0]}`;
    await waitFor(() => expect(screen.getByTestId(firstRowId)).toBeTruthy(), {
      timeout: 12_000,
    });

    for (const fixtureId of FIXTURE_IDS) {
      const row = screen.getByTestId(`ThreadRow-${FX_THREAD_PREFIX}${fixtureId}`);
      expect(row.textContent).toContain(FIXTURE_THREADS[fixtureId].title);
    }
  });

  it('given the fx-basic fixture row, when tapped, then the app pushes /coding-agent/fx-thread-fx-basic', async () => {
    const router = routerHarness();
    await renderList();

    const rowTestId = `ThreadRow-${FX_THREAD_PREFIX}fx-basic`;
    await waitFor(() => expect(screen.getByTestId(rowTestId)).toBeTruthy(), {
      timeout: 12_000,
    });

    await userEvent.click(screen.getByTestId(rowTestId));

    expect(router.lastPush()).toBe(`/coding-agent/${FX_THREAD_PREFIX}fx-basic`);
  });

  it('given the New thread button, when tapped, then the app creates a thread through the fixture gateway and pushes /coding-agent/<newId> where newId is not one of FIXTURE_IDS', async () => {
    const router = routerHarness();
    await renderList();

    const firstRowId = `ThreadRow-${FX_THREAD_PREFIX}${FIXTURE_IDS[0]}`;
    await waitFor(() => expect(screen.getByTestId(firstRowId)).toBeTruthy(), {
      timeout: 12_000,
    });

    await userEvent.click(screen.getByTestId('NewThreadButton'));

    await waitFor(() => {
      const lastPush = router.lastPush();
      expect(lastPush).toMatch(/^\/coding-agent\/[^/]+$/);
    }, { timeout: 12_000 });

    const lastPush = router.lastPush() as string;
    const newId = lastPush.replace(/^\/coding-agent\//, '');
    expect(FIXTURE_IDS).not.toContain(newId as (typeof FIXTURE_IDS)[number]);
  });
});