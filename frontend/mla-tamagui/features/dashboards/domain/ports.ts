import type {
  HeadroomResponse,
  MetricsCurrentResponse,
  MetricsSeriesResponse,
  PlanDetail,
  PlansResponse,
  RunReport,
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

export type DashboardsPorts = {
  reportingGateway: ReportingGateway;
};
