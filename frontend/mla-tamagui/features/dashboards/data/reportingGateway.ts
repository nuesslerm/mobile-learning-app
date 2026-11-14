// Reporting gateway for the agent dashboards: fetches the read-only
// reporting-service API (metrics, headroom, plans, runs) or serves in-memory
// fixtures. Both variants resolve to the same tagged union so the UI renders a
// distinct honest empty state for an unset URL and a failed fetch, exactly like
// frontend/mla-tamagui/features/service-report/src/data/ordsClient.ts. An unset URL is
// `not-configured`, never an error.

import { Logger } from '@lib/logging';
import { z } from 'zod';

import {
  headroomResponseSchema,
  metricsCurrentResponseSchema,
  metricsSeriesResponseSchema,
  planDetailSchema,
  plansResponseSchema,
  runReportSchema,
  type HeadroomResponse,
  type MetricsCurrentResponse,
  type MetricsSeriesResponse,
  type PlanDetail,
  type PlansResponse,
  type RunReport,
} from './reportingSchema';

export type ReportingResult<T> =
  | { status: 'not-configured' }
  | { status: 'unreachable'; reason: string }
  | { status: 'ready'; data: T };

export type SeriesWindow = '1h' | '24h' | '7d' | '30d';

export interface ReportingFixtures {
  metricsCurrent: MetricsCurrentResponse;
  metricsSeries: MetricsSeriesResponse;
  headroom: HeadroomResponse;
  plans: PlansResponse;
  planDetail: PlanDetail;
  run: RunReport;
}

export interface ReportingGateway {
  getMetricsCurrent(): Promise<ReportingResult<MetricsCurrentResponse>>;
  getMetricsSeries(window: SeriesWindow): Promise<ReportingResult<MetricsSeriesResponse>>;
  getHeadroom(): Promise<ReportingResult<HeadroomResponse>>;
  getPlans(): Promise<ReportingResult<PlansResponse>>;
  getPlanDetail(slug: string): Promise<ReportingResult<PlanDetail>>;
  getRun(runId: string): Promise<ReportingResult<RunReport>>;
}

type FetchLike = (
  input: string,
  init?: { method?: string },
) => Promise<Response>;

const ENV_BASE_URL_NAME = 'EXPO_PUBLIC_REPORTING_BASE_URL';

function stripTrailingSlash(url: string): string {
  return url.endsWith('/') ? url.slice(0, -1) : url;
}

async function fetchAndParse<T>(
  fetchImpl: FetchLike,
  url: string,
  op: string,
  schema: z.ZodType<T>,
): Promise<ReportingResult<T>> {
  let res: Response;
  try {
    res = await fetchImpl(url);
  } catch (cause) {
    Logger.withError(cause).withMetadata({ op, url }).warn(
      'dashboards.reporting.fetch_failed',
    );
    return { status: 'unreachable', reason: `${op}: fetch_failed` };
  }
  if (!res.ok) {
    Logger.withMetadata({ op, url, status: res.status }).warn(
      'dashboards.reporting.http_error',
    );
    return { status: 'unreachable', reason: `${op}: http_${res.status}` };
  }
  let body: unknown;
  try {
    body = await res.json();
  } catch (cause) {
    Logger.withError(cause).withMetadata({ op, url }).warn(
      'dashboards.reporting.parse_failed',
    );
    return { status: 'unreachable', reason: `${op}: parse_failed` };
  }
  const parsed = schema.safeParse(body);
  if (!parsed.success) {
    Logger.withMetadata({ op, url }).warn(
      'dashboards.reporting.schema_failed',
    );
    return { status: 'unreachable', reason: `${op}: schema_failed` };
  }
  Logger.withMetadata({ op, url }).info('dashboards.reporting.success');
  return { status: 'ready', data: parsed.data };
}

const notConfigured = <T>(): Promise<ReportingResult<T>> =>
  Promise.resolve({ status: 'not-configured' });

export function createHttpReportingGateway(
  baseUrl: string | undefined,
  fetchImpl: FetchLike = fetch as FetchLike,
): ReportingGateway {
  if (baseUrl === undefined || baseUrl.length === 0) {
    Logger.withMetadata({ reason: `${ENV_BASE_URL_NAME} is not set` }).info(
      'dashboards.reporting.not_configured',
    );
    return {
      getMetricsCurrent: () => notConfigured(),
      getMetricsSeries: () => notConfigured(),
      getHeadroom: () => notConfigured(),
      getPlans: () => notConfigured(),
      getPlanDetail: () => notConfigured(),
      getRun: () => notConfigured(),
    };
  }

  const base = stripTrailingSlash(baseUrl);
  return {
    getMetricsCurrent() {
      return fetchAndParse(
        fetchImpl,
        `${base}/api/metrics/current`,
        'getMetricsCurrent',
        metricsCurrentResponseSchema,
      );
    },
    getMetricsSeries(window) {
      return fetchAndParse(
        fetchImpl,
        `${base}/api/metrics/series?window=${window}`,
        'getMetricsSeries',
        metricsSeriesResponseSchema,
      );
    },
    getHeadroom() {
      return fetchAndParse(
        fetchImpl,
        `${base}/api/headroom`,
        'getHeadroom',
        headroomResponseSchema,
      );
    },
    getPlans() {
      return fetchAndParse(
        fetchImpl,
        `${base}/api/plans`,
        'getPlans',
        plansResponseSchema,
      );
    },
    getPlanDetail(slug) {
      return fetchAndParse(
        fetchImpl,
        `${base}/api/plans/${encodeURIComponent(slug)}`,
        'getPlanDetail',
        planDetailSchema,
      );
    },
    getRun(runId) {
      return fetchAndParse(
        fetchImpl,
        `${base}/api/runs/${encodeURIComponent(runId)}`,
        'getRun',
        runReportSchema,
      );
    },
  };
}

export function createFixtureReportingGateway(
  fixtures: ReportingFixtures,
): ReportingGateway {
  return {
    async getMetricsCurrent() {
      return { status: 'ready', data: fixtures.metricsCurrent };
    },
    async getMetricsSeries() {
      return { status: 'ready', data: fixtures.metricsSeries };
    },
    async getHeadroom() {
      return { status: 'ready', data: fixtures.headroom };
    },
    async getPlans() {
      return { status: 'ready', data: fixtures.plans };
    },
    async getPlanDetail() {
      return { status: 'ready', data: fixtures.planDetail };
    },
    async getRun() {
      return { status: 'ready', data: fixtures.run };
    },
  };
}