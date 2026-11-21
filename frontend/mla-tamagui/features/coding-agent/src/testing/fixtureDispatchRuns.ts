import { Logger } from '@lib/logging';

import type { DispatchRunDetail as DispatchRunDetailType, DispatchRunMeta } from '../domain';
import { DispatchRunsGatewayError, type DispatchRunsGateway } from '../domain/ports';
import { FIXTURE_DISPATCH_RUNS } from './fixtures';

function cloneDetail(detail: DispatchRunDetailType): DispatchRunDetailType {
  return JSON.parse(JSON.stringify(detail)) as DispatchRunDetailType;
}

function toMeta(detail: DispatchRunDetailType): DispatchRunMeta {
  const { lines: _lines, ...meta } = detail;
  return meta;
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
