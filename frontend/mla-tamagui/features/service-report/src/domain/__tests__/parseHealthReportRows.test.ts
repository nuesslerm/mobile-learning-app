import { describe, expect, it } from 'vitest';

import { parseHealthReportRows } from '../parseHealthReportRows';

function buildReport(generatedAt: string) {
  return {
    generatedAt,
    vmOps: { reachable: true, host: 'vm-ops-api-main', time: generatedAt },
    services: [{ app: 'leetcode-backend', reachable: true }],
    logs: { healthy: true, sources: [] },
    recentErrors: [],
  };
}

describe('given a mix of valid and invalid ORDS rows', () => {
  it('when parseHealthReportRows runs, then it parses valid rows and skips rows with invalid JSON', () => {
    const result = parseHealthReportRows([
      {
        id: 1,
        generated_at: '2026-08-10T00:00:00.000Z',
        report_json: JSON.stringify(buildReport('2026-08-10T00:00:00.000Z')),
      },
      {
        id: 2,
        generated_at: '2026-08-11T00:00:00.000Z',
        report_json: 'not json',
      },
    ]);

    expect(result.reports).toHaveLength(1);
    expect(result.reports[0].generatedAt).toBe('2026-08-10T00:00:00.000Z');
    expect(result.skippedRowCount).toBe(1);
  });

  it('when a row parses as JSON but fails schema validation, then it is skipped and counted', () => {
    const result = parseHealthReportRows([
      {
        id: 3,
        generated_at: '2026-08-12T00:00:00.000Z',
        report_json: JSON.stringify({ not: 'a health report' }),
      },
    ]);

    expect(result.reports).toEqual([]);
    expect(result.skippedRowCount).toBe(1);
  });

  it('when all rows are valid, then nothing is skipped', () => {
    const result = parseHealthReportRows([
      {
        id: 4,
        generated_at: '2026-08-13T00:00:00.000Z',
        report_json: JSON.stringify(buildReport('2026-08-13T00:00:00.000Z')),
      },
    ]);

    expect(result.reports).toHaveLength(1);
    expect(result.skippedRowCount).toBe(0);
  });

  it('when given no rows, then it returns an empty result', () => {
    expect(parseHealthReportRows([])).toEqual({
      reports: [],
      skippedRowCount: 0,
    });
  });
});
