import type { GoldenScreenshotComment } from './goldenScreenshotComment';
import type { HealthReport } from './healthReport';

export type HealthReportHistoryResult =
  | { status: 'not-configured'; reason: string }
  | { status: 'unreachable'; error: string }
  | { status: 'ready'; reports: HealthReport[]; skippedRowCount: number };

export type GoldenScreenshotCommentsResult =
  | { status: 'not-configured'; reason: string }
  | { status: 'unreachable'; error: string }
  | { status: 'ready'; comments: GoldenScreenshotComment[] };

export type PostGoldenScreenshotCommentResult =
  | { status: 'not-configured'; reason: string }
  | { status: 'error'; error: string }
  | { status: 'ok' };

/** The ORDS endpoint and its basic-auth credentials; absent while the health-reports database is not provisioned. */
export type OrdsConnection = { baseUrl: string; user: string; password: string };

export type HealthReportsPort = {
  fetchHistory(): Promise<HealthReportHistoryResult>;
};

export type GoldenCommentsPort = {
  fetch(screenshotKey: string): Promise<GoldenScreenshotCommentsResult>;
  post(input: { screenshotKey: string; author?: string; commentText: string }): Promise<PostGoldenScreenshotCommentResult>;
};

export type ServiceReportPorts = {
  healthReports: HealthReportsPort;
  goldenComments: GoldenCommentsPort;
};
