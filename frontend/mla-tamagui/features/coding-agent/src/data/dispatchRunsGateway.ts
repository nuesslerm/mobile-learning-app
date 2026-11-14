import {
  DispatchRunDetail,
  DispatchRunsListResponse,
} from '../domain';
import { Logger } from '@lib/logging';

import { DispatchRunsGatewayError, type DispatchRunsGateway, type DispatchRunsPort } from '../domain/ports';

type FetchLike = (
  input: string,
  init?: { method?: string },
) => Promise<Response>;

function stripTrailingSlash(url: string): string {
  return url.endsWith('/') ? url.slice(0, -1) : url;
}

function buildListUrl(baseUrl: string, filter: { agent?: string; q?: string } | undefined): string {
  const params = new URLSearchParams();
  if (filter?.agent !== undefined && filter.agent.length > 0) {
    params.set('agent', filter.agent);
  }
  if (filter?.q !== undefined && filter.q.length > 0) {
    params.set('q', filter.q);
  }
  const qs = params.toString();
  const base = stripTrailingSlash(baseUrl);
  return qs.length > 0
    ? `${base}/opencode-log/api/runs?${qs}`
    : `${base}/opencode-log/api/runs`;
}

async function fetchAndParse<T>(
  fetchImpl: FetchLike,
  url: string,
  op: string,
  schema: { parse: (value: unknown) => T },
): Promise<T> {
  let res: Response;
  try {
    res = await fetchImpl(url);
  } catch (cause) {
    Logger.withError(cause).withMetadata({ op, url }).warn(
      'coding-agent.dispatch_runs.fetch_failed',
    );
    throw new DispatchRunsGatewayError(`${op}: fetch_failed`, 0);
  }
  if (!res.ok) {
    Logger.withMetadata({ op, url, status: res.status }).warn(
      'coding-agent.dispatch_runs.http_error',
    );
    throw new DispatchRunsGatewayError(`${op}: http_${res.status}`, res.status);
  }
  let body: unknown;
  try {
    body = await res.json();
  } catch (cause) {
    Logger.withError(cause).withMetadata({ op, url }).warn(
      'coding-agent.dispatch_runs.parse_failed',
    );
    throw new DispatchRunsGatewayError(`${op}: parse_failed`, 0);
  }
  try {
    return schema.parse(body);
  } catch (cause) {
    Logger.withError(cause).withMetadata({ op, url }).warn(
      'coding-agent.dispatch_runs.schema_failed',
    );
    throw new DispatchRunsGatewayError(`${op}: schema_failed`, 0);
  }
}

export function createHttpDispatchRunsGateway(
  baseUrl: string,
  fetchImpl: FetchLike = fetch as FetchLike,
): DispatchRunsGateway {
  return {
    async listRuns(filter) {
      const url = buildListUrl(baseUrl, filter);
      const parsed = await fetchAndParse(
        fetchImpl,
        url,
        'listRuns',
        DispatchRunsListResponse,
      );
      return parsed.runs;
    },
    async getRun(runId) {
      const base = stripTrailingSlash(baseUrl);
      const url = `${base}/opencode-log/api/runs/${encodeURIComponent(runId)}`;
      return fetchAndParse(fetchImpl, url, 'getRun', DispatchRunDetail);
    },
  };
}

export function dispatchRunDashboardUrl(baseUrl: string, runId: string): string {
  return `${stripTrailingSlash(baseUrl)}/opencode-log/${encodeURIComponent(runId)}`;
}

/** The dispatch-runs port for a reporting base URL; without one the section hides and a run screen stays empty. */
export function createHttpDispatchRunsPort(baseUrl: string | undefined): DispatchRunsPort {
  if (baseUrl === undefined) return { gateway: undefined, dashboardUrl: () => undefined };
  const base = stripTrailingSlash(baseUrl);
  return {
    gateway: createHttpDispatchRunsGateway(base),
    dashboardUrl: (runId) => dispatchRunDashboardUrl(base, runId),
  };
}
