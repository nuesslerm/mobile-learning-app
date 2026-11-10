import { screen, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../__tests__/flowApp';

function renderScreen(fetchHealthReportHistory: ReturnType<typeof vi.fn>) {
  return app.render('/service-report', {
    overrides: { 'service-report': { fetchHealthReportHistory } },
  });
}

describe('ServiceReportScreen flow (health-report fetch)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('given the health-report fetch returns data, when the screen renders, then the latest snapshot card and service rows display', async () => {
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

  it('given the health-report fetch fails with unreachable endpoint, when the screen renders, then the error admonition displays', async () => {
    const fetchHealthReportHistory = vi
      .fn()
      .mockRejectedValueOnce(
        new Error('Network error: Could not reach endpoint')
      );

    await renderScreen(fetchHealthReportHistory);

    await waitFor(() => {
      expect(
        screen.getByTestId('ServiceReportScreen-UnreachableAdmonition')
      ).toBeTruthy();
    });
    expect(
      screen.getByTestId('ServiceReportScreen-UnreachableAdmonition')
        .textContent
    ).toContain('Could not reach');
  });
});
