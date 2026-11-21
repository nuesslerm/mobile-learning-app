/**
 * Fetches the health-report history from the Oracle ORDS auto-REST
 * endpoint published by `wf check --health --publish`.
 *
 * The Autonomous DB has not been provisioned yet as of writing this, so
 * there is no real endpoint to reach — `EXPO_PUBLIC_HEALTH_REPORTS_ORDS_BASE_URL`
 * / `_PASSWORD` are expected to be unset in every environment until a later
 * phase wires up the live database. That is treated as a normal, expected
 * state (`status: 'not-configured'`), never thrown as an error — same for a
 * real fetch failure (`status: 'unreachable'`), so the UI can render a
 * distinct, honest empty-state message for each rather than crashing or
 * showing a raw error screen.
 */

import { Logger } from '@lib/logging';
import { sortReportsByGeneratedAtDesc } from '../domain';
import { z } from 'zod';

import { encodeBase64 } from '../domain/base64';
import type { HealthReportHistoryResult, HealthReportsPort, OrdsConnection } from '../domain/ports';
import {
  ordsRowSchema,
  parseHealthReportRows,
} from '../domain/parseHealthReportRows';

const ENV_BASE_URL_NAME = 'EXPO_PUBLIC_HEALTH_REPORTS_ORDS_BASE_URL';
const ENV_PASSWORD_NAME = 'EXPO_PUBLIC_HEALTH_REPORTS_ORDS_PASSWORD';

const ordsEnvelopeSchema = z.object({
  items: z.array(ordsRowSchema),
  hasMore: z.boolean().optional(),
  limit: z.number().optional(),
  offset: z.number().optional(),
  count: z.number().optional(),
});

function authHeader(config: OrdsConnection): string {
  return `Basic ${encodeBase64(`${config.user}:${config.password}`)}`;
}

async function fetchHealthReportHistory(
  config: OrdsConnection | undefined,
  fetchImpl: typeof fetch
): Promise<HealthReportHistoryResult> {
  if (!config) {
    Logger.info('service_report.fetch_not_configured');
    return {
      status: 'not-configured',
      reason: `${ENV_BASE_URL_NAME} / ${ENV_PASSWORD_NAME} are not set — the health-reports Autonomous DB has not been provisioned yet.`,
    };
  }

  Logger.info('service_report.fetch_start', { baseUrl: config.baseUrl });

  let response: Response;
  try {
    response = await fetchImpl(config.baseUrl, {
      method: 'GET',
      headers: { Authorization: authHeader(config) },
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    Logger.warn('service_report.fetch_failed', { err });
    return { status: 'unreachable', error: message };
  }

  if (!response.ok) {
    const message = `ORDS request failed (HTTP ${response.status})`;
    Logger.warn('service_report.fetch_failed', { status: response.status });
    return { status: 'unreachable', error: message };
  }

  let envelopeJson: unknown;
  try {
    envelopeJson = await response.json();
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    Logger.warn('service_report.envelope_parse_failed', { err });
    return {
      status: 'unreachable',
      error: `Could not parse ORDS response: ${message}`,
    };
  }

  const envelope = ordsEnvelopeSchema.safeParse(envelopeJson);
  if (!envelope.success) {
    Logger.warn('service_report.envelope_schema_invalid', {
      issues: envelope.error.issues,
    });
    return {
      status: 'unreachable',
      error:
        'ORDS response did not match the expected auto-REST envelope shape.',
    };
  }

  const { reports, skippedRowCount } = parseHealthReportRows(
    envelope.data.items
  );
  const sorted = sortReportsByGeneratedAtDesc(reports);

  Logger.info('service_report.fetch_success', {
    reportCount: sorted.length,
    skippedRowCount,
  });

  return { status: 'ready', reports: sorted, skippedRowCount };
}

export function createOrdsHealthReports(config: OrdsConnection | undefined, fetchImpl: typeof fetch = fetch): HealthReportsPort {
  return { fetchHistory: () => fetchHealthReportHistory(config, fetchImpl) };
}
