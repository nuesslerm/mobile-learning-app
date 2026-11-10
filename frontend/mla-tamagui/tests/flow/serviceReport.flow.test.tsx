import { beforeEach, describe, expect, it, vi } from 'vitest';

import { screen, userEvent, waitFor } from '@lib/flow-test-harness';

import { app } from './app';

function renderScreen(fetchHealthReportHistory: ReturnType<typeof vi.fn>) {
  return app.render('/service-report', {
    overrides: { 'service-report': { fetchHealthReportHistory } },
  });
}

function oneServiceReport() {
  return {
    status: 'ready' as const,
    skippedRowCount: 0,
    reports: [
      {
        generatedAt: '2026-09-09T12:00:00Z',
        vmOps: {
          reachable: true as const,
          host: 'main',
          time: '2026-09-09T12:00:00Z',
        },
        services: [
          {
            app: 'reporting-service',
            reachable: true,
            containers: [
              {
                name: 'bot',
                state: 'running',
                status: 'healthy',
                image: 'bot:latest',
              },
            ],
          },
        ],
        logs: { healthy: true, sources: [] },
        recentErrors: [],
        recentLogLines: [],
        recentSpans: [],
      },
    ],
  };
}

describe('ServiceReportScreen flow (health-report fetch stubbed)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('given the health-report fetch returns data, when the screen renders, then the summary card and service rows display', async () => {
    const mockData = {
      status: 'ready' as const,
      skippedRowCount: 0,
      reports: [
        {
          generatedAt: '2026-09-09T12:00:00Z',
          vmOps: {
            reachable: true as const,
            host: 'main',
            time: '2026-09-09T12:00:00Z',
          },
          services: [
            {
              app: 'leetcode-backend',
              reachable: true,
              containers: [
                {
                  name: 'backend',
                  state: 'running',
                  status: 'healthy',
                  image: 'leetcode:latest',
                },
              ],
            },
          ],
          logs: { healthy: true, sources: [] },
          recentErrors: [],
          recentLogLines: [],
          recentSpans: [],
        },
      ],
    };

    const fetchHealthReportHistory = vi.fn().mockResolvedValueOnce(mockData);

    await renderScreen(fetchHealthReportHistory);

    await waitFor(
      () => {
        const loading = screen.queryByTestId(
          'ServiceReportScreen-LoadingIndicator'
        );
        expect(loading).toBeFalsy();
      },
      { timeout: 5000 }
    );

    expect(screen.getByTestId('ServiceReportScreen-SummaryCard')).toBeTruthy();

    expect(
      screen.getByTestId('ServiceReportScreen-ServiceRow-leetcode-backend')
    ).toBeTruthy();
  });

  it('given a service row rendered, when tapped, then the drill dialog opens and displays the service details', async () => {
    const mockData = oneServiceReport();

    const fetchHealthReportHistory = vi.fn().mockResolvedValueOnce(mockData);

    await renderScreen(fetchHealthReportHistory);

    await waitFor(
      () => {
        expect(
          screen.getByTestId('ServiceReportScreen-ServiceRow-reporting-service')
        ).toBeTruthy();
      },
      { timeout: 5000 }
    );

    await userEvent.click(
      screen.getByTestId('ServiceReportScreen-ServiceRow-reporting-service')
    );

    await waitFor(() => {
      expect(
        screen.getByTestId('ServiceReportScreen-DrillDialog-Title').textContent
      ).toBe('reporting-service — uptime history');
    });
  });

  it('given the uptime chart rendered, when its bar is tapped, then the drill dialog opens for that service', async () => {
    // Given one service's uptime drawn as one bar
    await renderScreen(vi.fn().mockResolvedValueOnce(oneServiceReport()));
    const bar = await waitFor(
      () =>
        screen.getByTestId('ServiceReportScreen-UptimeChart-BarChart-Bar-0'),
      { timeout: 5000 }
    );

    // When the bar is tapped
    await userEvent.click(bar);

    // Then the drill dialog shows that service's uptime history
    await waitFor(() => {
      expect(
        screen.getByTestId('ServiceReportScreen-DrillDialog-Title').textContent
      ).toBe('reporting-service — uptime history');
    });
  });
});
