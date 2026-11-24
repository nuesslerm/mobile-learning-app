import { describe, expect, it } from 'vitest';

import {
  logLevelCounts,
  serviceUptimeSummary,
  type HealthReport,
  type LogLine,
} from '..';

describe('given services whose reachability changes across snapshots', () => {
  it('when serviceUptimeSummary runs, then it tracks first-seen order and guards the zero-total case', () => {
    const reports = [
      { services: [{ app: 'svc-a', reachable: true }] },
      { services: [{ app: 'svc-a', reachable: false }] },
      { services: [] },
    ] as unknown as HealthReport[];

    expect(serviceUptimeSummary(reports)).toEqual([
      { app: 'svc-a', upCount: 1, totalCount: 2, upPercent: 50 },
    ]);
  });
});

describe('given a mixed-level list of recent log lines', () => {
  it('when logLevelCounts runs, then it normalizes case, counts, and sorts most-frequent first', () => {
    const lines = [
      { level: 'info' },
      { level: 'INFO' },
      { level: 'warn' },
      { level: 'error' },
    ] as unknown as LogLine[];

    expect(logLevelCounts(lines)).toEqual([
      { level: 'info', count: 2 },
      { level: 'warn', count: 1 },
      { level: 'error', count: 1 },
    ]);
  });
});
