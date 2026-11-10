import { appConfig } from '@lib/app-config';
import {
  DispatchRunDetail,
  DispatchRunsListResponse,
  FIXTURE_DISPATCH_RUNS,
  type DispatchRunDetail as DispatchRunDetailType,
  type DispatchRunMeta,
} from '../domain';
import { Logger } from '@lib/logging';

export class DispatchRunsGatewayError extends Error {
  readonly status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = 'DispatchRunsGatewayError';
    this.status = status;
  }
}

export interface DispatchRunsGateway {
  listRuns(filter?: { agent?: string; q?: string }): Promise<DispatchRunMeta[]>;
  getRun(runId: string): Promise<DispatchRunDetailType>;
}

type FetchLike = (
  input: string,
  init?: { method?: string },
) => Promise<Response>;

function stripTrailingSlash(url: string): string {
  return url.endsWith('/') ? url.slice(0, -1) : url;
}

function cloneDetail(detail: DispatchRunDetailType): DispatchRunDetailType {
  return JSON.parse(JSON.stringify(detail)) as DispatchRunDetailType;
}

function toMeta(detail: DispatchRunDetailType): DispatchRunMeta {
  const { lines: _lines, ...meta } = detail;
  return meta;
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

const RUNNING_LINE_TARGET = 12;
const RUNNING_LINES_PER_POLL = 2;
const FINISHED_COST = 0.0042;
const FINISHED_OK = true;

function appendStreamLine(lines: string[], step: number): void {
  lines.push(
    JSON.stringify({
      type: 'text',
      text: `[stream] step ${step}: still working on the task.`,
    }),
  );
}

export function createFixtureDispatchRunsGateway(): DispatchRunsGateway {
  const finished: DispatchRunDetailType = cloneDetail(
    FIXTURE_DISPATCH_RUNS.finished,
  );
  const state: { running: DispatchRunDetailType } = {
    running: cloneDetail(FIXTURE_DISPATCH_RUNS.running),
  };

  const nextRunning = (): DispatchRunDetailType => {
    const lines = state.running.lines;
    if (lines.length < RUNNING_LINE_TARGET) {
      const slot = Math.min(
        RUNNING_LINES_PER_POLL,
        RUNNING_LINE_TARGET - lines.length,
      );
      const start = lines.length + 1;
      for (let i = 0; i < slot; i += 1) {
        appendStreamLine(lines, start + i);
      }
    }
    if (lines.length >= RUNNING_LINE_TARGET) {
      const finishedAt = new Date().toISOString();
      const next: DispatchRunDetailType = {
        ...state.running,
        status: 'finished',
        finishedAt,
        ok: FINISHED_OK,
        cost: FINISHED_COST,
        durationMs:
          new Date(finishedAt).getTime() -
          new Date(state.running.startedAt).getTime(),
      };
      state.running = next;
    }
    return cloneDetail(state.running);
  };

  return {
    async listRuns() {
      return [toMeta(cloneDetail(finished)), toMeta(nextRunning())];
    },
    async getRun(runId) {
      if (runId === finished.runId) {
        return cloneDetail(finished);
      }
      if (runId === state.running.runId) {
        return nextRunning();
      }
      Logger.withMetadata({ runId }).warn(
        'coding-agent.dispatch_runs.run_not_found',
      );
      throw new DispatchRunsGatewayError(`getRun: not_found`, 404);
    },
  };
}

export function resolveVmOpsApiBaseUrl(): string | undefined {
  const value = appConfig.vmOpsApiBaseUrl;
  if (value === undefined) return undefined;
  return stripTrailingSlash(value);
}

export function dispatchRunDashboardUrl(baseUrl: string, runId: string): string {
  return `${stripTrailingSlash(baseUrl)}/opencode-log/${encodeURIComponent(runId)}`;
}
