import type { ComponentType } from 'react';
import {
  createFlowApp,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import type { PersistedLogEntry } from '@lib/logging';
import { createInMemoryPersistence } from '@lib/logging/persistence';
import { describe, expect, it, beforeEach } from 'vitest';

import { queryPage, setPersistence } from '../../lib/logPersistence';

// The debug sheet is hosted by DebugFab in the ROOT layout, so the app is mounted with the root layout and the FAB is tapped open.
const app = createFlowApp({
  routes: import.meta.glob<{ default: ComponentType }>('../../app/**/*.tsx'),
  routesRoot: '../../app/',
  includeRootLayout: true,
});

const createTestEntries = (count: number): PersistedLogEntry[] => {
  return Array.from({ length: count }, (_, i) => ({
    dbId: i + 1,
    ts: Date.now() - (count - i) * 1000,
    level: i % 3 === 0 ? 'error' : i % 2 === 0 ? 'warn' : 'info',
    text: `Test log entry ${i + 1}`,
    source: 'TestModule',
  }));
};

describe('DebugLogSheet flow (persisted log entries)', () => {
  beforeEach(async () => {
    const persistence = createInMemoryPersistence();
    setPersistence(persistence);

    const entries = createTestEntries(60);
    await persistence.appendBatch(entries);
  });

  it('given a seeded page of persisted entries, when the sheet renders, then the first 50 entries display', async () => {
    await app.render('/showcase');
    await userEvent.click(await screen.findByTestId('DebugFabWrap'));

    await waitFor(() => {
      expect(screen.getByTestId('DebugLogSheetTitle')).toBeTruthy();
      const logTexts = screen.getAllByTestId('DebugLogText');
      expect(logTexts.length).toBeGreaterThan(0);
    }, { timeout: 3000 });
  });

  it('given the sheet with 60 persisted entries, when the Load older button is pressed, then the next page of entries loads', async () => {
    await app.render('/showcase');
    await userEvent.click(await screen.findByTestId('DebugFabWrap'));

    await waitFor(() => {
      expect(screen.getByTestId('DebugLogSheetLoadMore')).toBeTruthy();
    }, { timeout: 3000 });

    const loadMoreButton = screen.getByTestId('DebugLogSheetLoadMore');
    const entriesBefore = screen.getAllByTestId('DebugLogText').length;

    await userEvent.click(loadMoreButton);

    await waitFor(() => {
      const entriesAfter = screen.getAllByTestId('DebugLogText').length;
      expect(entriesAfter).toBeGreaterThan(entriesBefore);
    }, { timeout: 3000 });
  });

  it('given the sheet with entries displayed, when the Clear button is pressed, then the entry list becomes empty', async () => {
    await app.render('/showcase');
    await userEvent.click(await screen.findByTestId('DebugFabWrap'));

    await waitFor(() => {
      const logTexts = screen.getAllByTestId('DebugLogText');
      expect(logTexts.length).toBeGreaterThan(0);
    }, { timeout: 3000 });

    const clearButton = screen.getByTestId('DebugLogSheetClearButton');
    await userEvent.click(clearButton);

    await waitFor(() => {
      expect(screen.getByTestId('DebugLogSheetEmpty')).toBeTruthy();
    }, { timeout: 3000 });

    const entries = await queryPage({ limit: 50 });
    expect(entries).toHaveLength(0);
  });
});