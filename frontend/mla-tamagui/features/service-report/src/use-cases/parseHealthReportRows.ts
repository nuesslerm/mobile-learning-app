import { Logger } from '@lib/logging';
import {
  healthReportSchema,
  type HealthReport,
} from '../contracts';
import { z } from 'zod';

export const ordsRowSchema = z.object({
  id: z.number(),
  generated_at: z.string(),
  report_json: z.string(),
});

export type OrdsRow = {
  id: number;
  generated_at: string;
  report_json: string;
};

/**
 * Parses each row's `report_json` string, validating against
 * `healthReportSchema` — a row that fails to parse (e.g. a stale/partial
 * shape from a future or past `HealthReport` version) is skipped and
 * logged rather than failing the whole list.
 */
export function parseHealthReportRows(rows: OrdsRow[]): {
  reports: HealthReport[];
  skippedRowCount: number;
} {
  const reports: HealthReport[] = [];
  let skippedRowCount = 0;
  for (const row of rows) {
    let parsedJson: unknown;
    try {
      parsedJson = JSON.parse(row.report_json);
    } catch (err) {
      skippedRowCount += 1;
      Logger.warn('service_report.row_json_parse_failed', {
        rowId: row.id,
        err,
      });
      continue;
    }
    const result = healthReportSchema.safeParse(parsedJson);
    if (!result.success) {
      skippedRowCount += 1;
      Logger.warn('service_report.row_schema_invalid', {
        rowId: row.id,
        issues: result.error.issues,
      });
      continue;
    }
    reports.push(result.data);
  }
  return { reports, skippedRowCount };
}
