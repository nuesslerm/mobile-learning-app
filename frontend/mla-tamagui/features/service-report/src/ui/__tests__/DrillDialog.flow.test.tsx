import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../__tests__/flowApp';

// Mock AlertDialog to avoid web-specific findNodeHandle issue in gluestack

const UPTIME_REPORT_1 = {
  generatedAt: '2026-09-09T12:00:00Z',
  vmOps: {
    reachable: true,
    host: 'main',
    time: '2026-09-09T12:00:00Z',
  },
  services: [
    {
      app: 'api-gateway',
      reachable: true,
      containers: [
        {
          name: 'gateway',
          state: 'running',
          status: 'healthy',
          image: 'gateway:latest',
        },
      ],
    },
  ],
  logs: { healthy: true, sources: [] },
  recentErrors: [],
  recentLogLines: [],
  recentSpans: [],
};

const UPTIME_REPORT_2 = {
  generatedAt: '2026-09-08T12:00:00Z',
  vmOps: {
    reachable: true,
    host: 'main',
    time: '2026-09-08T12:00:00Z',
  },
  services: [{ app: 'api-gateway', reachable: false, error: 'timeout' }],
  logs: { healthy: true, sources: [] },
  recentErrors: [],
  recentLogLines: [],
  recentSpans: [],
};

function renderScreen(fetchHealthReportHistory: ReturnType<typeof vi.fn>) {
  return app.render('/service-report', {
    overrides: { 'service-report': { fetchHealthReportHistory } },
  });
}

async function openDrillDialog() {
  const fetchHealthReportHistory = vi.fn().mockResolvedValueOnce({
    status: 'ready',
    skippedRowCount: 0,
    reports: [UPTIME_REPORT_1, UPTIME_REPORT_2],
  });

  await renderScreen(fetchHealthReportHistory);

  await waitFor(
    () => {
      expect(
        screen.getByTestId('ServiceReportScreen-ServiceRow-api-gateway')
      ).toBeTruthy();
    },
    { timeout: 5000 }
  );

  await userEvent.click(
    screen.getByTestId('ServiceReportScreen-ServiceRow-api-gateway')
  );

  await waitFor(() => {
    expect(
      screen.getByTestId('ServiceReportScreen-DrillDialog-Content')
    ).toBeTruthy();
  });

  return fetchHealthReportHistory;
}

describe('DrillDialog flow', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('given the dialog is open with items, when the screen renders, then the items render and display', async () => {
    await openDrillDialog();

    expect(
      screen.getByTestId(
        'ServiceReportScreen-DrillDialog-Item-api-gateway-2026-09-09T12:00:00Z'
      )
    ).toBeTruthy();

    expect(
      screen.getByTestId(
        'ServiceReportScreen-DrillDialog-Item-api-gateway-2026-09-08T12:00:00Z'
      )
    ).toBeTruthy();
  });

  it('given the dialog is open with items, when the close action button is pressed, then the dialog closes', async () => {
    await openDrillDialog();

    const closeButton = screen.getByTestId(
      'ServiceReportScreen-DrillDialog-Close'
    );
    await userEvent.click(closeButton);

    await waitFor(() => {
      expect(
        screen.queryByTestId('ServiceReportScreen-DrillDialog-Content')
      ).toBeFalsy();
    });
  });
});
