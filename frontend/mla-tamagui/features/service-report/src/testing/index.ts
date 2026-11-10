import {
  sortReportsByGeneratedAtDesc,
  type HealthReport,
} from '../contracts';

import type { HealthReportHistoryResult } from '../data/ordsClient';

/**
 * Static fixture data for tests. Stands in for a real ORDS fetch so
 * `useHealthReportHistory` renders something representative under
 * `DependencyOverridesProvider`.
 */
const FIXTURE_HEALTH_REPORTS: HealthReport[] = [
  {
    generatedAt: '2026-08-13T09:00:00.000Z',
    vmOps: {
      reachable: true,
      host: 'vm-ops-api-main',
      time: '2026-08-13T09:00:00.000Z',
    },
    services: [
      {
        app: 'leetcode-backend',
        reachable: true,
        containers: [
          {
            name: 'leetcode-backend-main',
            state: 'running',
            status: 'Up 3 days',
            image: 'leetcode-backend:latest',
          },
        ],
      },
      {
        app: 'reporting-service',
        reachable: true,
        containers: [
          {
            name: 'reporting-service-main',
            state: 'running',
            status: 'Up 3 days',
            image: 'reporting-service:latest',
          },
        ],
      },
      {
        app: 'vm-gateway',
        reachable: true,
        containers: [
          {
            name: 'vm-gateway-main',
            state: 'running',
            status: 'Up 3 days',
            image: 'vm-gateway:latest',
          },
        ],
      },
    ],
    logs: {
      healthy: true,
      sources: [
        {
          source: 'backend',
          newestRowIso: '2026-08-13T08:59:30.000Z',
          rowCount: 512,
          live: true,
        },
        {
          source: 'mobile',
          newestRowIso: '2026-08-13T07:12:00.000Z',
          rowCount: 3,
          live: true,
        },
      ],
    },
    recentErrors: [
      { source: 'backend', windowHours: 24, errorCount: 1, sampleMessages: [] },
      { source: 'mobile', windowHours: 24, errorCount: 0, sampleMessages: [] },
    ],
    recentLogLines: [],
    recentSpans: [],
  },
  {
    generatedAt: '2026-08-14T09:00:00.000Z',
    vmOps: {
      reachable: false,
      error: 'ETIMEDOUT connecting to vm-ops-api-main:8080',
    },
    services: [
      {
        app: 'leetcode-backend',
        reachable: false,
        error: 'vm-ops-api unreachable',
      },
      {
        app: 'reporting-service',
        reachable: false,
        error: 'vm-ops-api unreachable',
      },
      { app: 'vm-gateway', reachable: false, error: 'vm-ops-api unreachable' },
    ],
    logs: {
      healthy: false,
      sources: [
        {
          source: 'backend',
          newestRowIso: '2026-08-14T08:40:00.000Z',
          rowCount: 90,
          live: true,
        },
        {
          source: 'mobile',
          newestRowIso: null,
          rowCount: 0,
          live: false,
          error: 'no rows in freshness window',
        },
      ],
    },
    recentErrors: [
      {
        source: 'backend',
        windowHours: 24,
        errorCount: 14,
        sampleMessages: ['vm-ops-api unreachable'],
      },
      { source: 'mobile', windowHours: 24, errorCount: 0, sampleMessages: [] },
    ],
    recentLogLines: [],
    recentSpans: [],
  },
];

export function createFixtureFetchHealthReportHistory(): () => Promise<HealthReportHistoryResult> {
  return async () => ({
    status: 'ready',
    reports: sortReportsByGeneratedAtDesc(FIXTURE_HEALTH_REPORTS),
    skippedRowCount: 0,
  });
}