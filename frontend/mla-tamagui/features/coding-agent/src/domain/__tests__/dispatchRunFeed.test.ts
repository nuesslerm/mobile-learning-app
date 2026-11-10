// features/coding-agent/src/domain/__tests__/dispatchRunFeed.test.ts
//
// Given/When/Then coverage for `buildDispatchRunFeed`: finished, running, and
// ok=false runs; coalesced text, collected reasoning, work-log status from
// output, trailing markers, and id determinism. Pure contracts package, no IO.

import { describe, expect, it } from 'vitest';

import {
  type DispatchRunDetail,
  type DispatchRunLine,
} from '../../contracts/dispatchRuns';
import { buildDispatchRunFeed } from '../dispatchRunFeed';

function makeDetail(
  overrides: Partial<{
    runId: string;
    prompt: string;
    startedAt: string;
    status: 'running' | 'finished';
    ok: boolean | null;
    lines: string[];
  }> = {}
): DispatchRunDetail {
  return {
    runId: overrides.runId ?? 'run-1',
    prompt: overrides.prompt ?? 'Fix the bug',
    agent: null,
    caller: null,
    claudeSessionId: null,
    model: null,
    startedAt: overrides.startedAt ?? '2026-04-01T00:00:00.000Z',
    finishedAt: overrides.status === 'finished' ? '2026-04-01T00:00:10.000Z' : null,
    status: overrides.status ?? 'finished',
    ok: overrides.ok === undefined ? true : overrides.ok,
    tokens: null,
    cost: null,
    durationMs: null,
    lines: overrides.lines ?? [],
  };
}

function jsonLine(line: DispatchRunLine): string {
  return JSON.stringify(line);
}

describe('buildDispatchRunFeed', () => {
  it('given a finished run with text, reasoning, 3 tool_use, step_finish, when built, then it emits 1 user, 1 assistant, 3 work-log, no working, and collected reasoning', () => {
    // Given a finished run with text, reasoning, 3 tool_use, and a step_finish
    const detail = makeDetail({
      runId: 'run-finished',
      prompt: 'Refactor the parser',
      lines: [
        jsonLine({ type: 'text', text: 'First paragraph of the answer.' }),
        jsonLine({ type: 'reasoning', text: 'Thinking about the shape.' }),
        jsonLine({ type: 'text', text: 'Second paragraph after reasoning.' }),
        jsonLine({ type: 'tool_use', name: 'read_file', input: { path: 'a.ts' } }),
        jsonLine({
          type: 'tool_use',
          name: 'bash',
          input: { command: 'pnpm test' },
          output: { stdout: 'ok' },
        }),
        jsonLine({
          type: 'tool_use',
          name: 'edit',
          input: { path: 'b.ts' },
          output: 'wrote 3 lines',
        }),
        jsonLine({ type: 'step_finish' }),
      ],
    });
    // When built
    const result = buildDispatchRunFeed(detail);
    // Then exactly one user-message, one assistant-message, three work-log entries, no working, and the reasoning is collected
    const kinds = result.feed.map((entry) => entry.kind);
    expect(kinds.filter((kind) => kind === 'user-message')).toHaveLength(1);
    expect(kinds.filter((kind) => kind === 'assistant-message')).toHaveLength(1);
    expect(kinds.filter((kind) => kind === 'work-log')).toHaveLength(3);
    expect(kinds.filter((kind) => kind === 'working')).toHaveLength(0);
    expect(result.feed.some((entry) => entry.kind === 'working')).toBe(false);
    expect(result.streamingTurnId).toBeUndefined();
    expect(result.reasoningByTurnId).toEqual({
      'run:run-finished': 'Thinking about the shape.',
    });
  });

  it('given a running run, when built, then the last entry is a working entry and streamingTurnId is set', () => {
    // Given a running run with two tool_use lines
    const detail = makeDetail({
      runId: 'run-running',
      prompt: 'Investigate',
      status: 'running',
      ok: null,
      lines: [
        jsonLine({ type: 'tool_use', name: 'read_file', input: { path: 'x.ts' } }),
        jsonLine({ type: 'tool_use', name: 'bash', input: { command: 'ls' } }),
      ],
    });
    // When built
    const result = buildDispatchRunFeed(detail);
    // Then the last entry is the working marker and streamingTurnId is set
    expect(result.feed.at(-1)).toMatchObject({
      kind: 'working',
      turnId: 'run:run-running',
    });
    expect(result.streamingTurnId).toBe('run:run-running');
  });

  it('given a finished run with ok=false, when built, then the last entry is an error entry', () => {
    // Given a finished run whose ok flag is false
    const detail = makeDetail({
      runId: 'run-failed',
      prompt: 'Try the migration',
      status: 'finished',
      ok: false,
      lines: [
        jsonLine({ type: 'text', text: 'Started.' }),
        jsonLine({
          type: 'tool_use',
          name: 'bash',
          input: { command: 'pnpm migrate' },
          output: 'Error: connection refused',
        }),
      ],
    });
    // When built
    const result = buildDispatchRunFeed(detail);
    // Then the last entry is an error entry for the dispatch run
    expect(result.feed.at(-1)).toMatchObject({
      kind: 'error',
      turnId: 'run:run-failed',
    });
    expect(result.streamingTurnId).toBeUndefined();
  });

  it('given the same detail twice, when built twice, then the feed ids are deterministic', () => {
    // Given the same detail built twice
    const detail = makeDetail({
      runId: 'run-determinism',
      prompt: 'Determinism check',
      lines: [
        jsonLine({ type: 'text', text: 'hi' }),
        jsonLine({ type: 'tool_use', name: 'read_file', input: { path: 'a' } }),
        jsonLine({ type: 'tool_use', name: 'bash', input: { command: 'ls' }, output: 'ok' }),
      ],
    });
    // When built twice
    const first = buildDispatchRunFeed(detail);
    const second = buildDispatchRunFeed(detail);
    // Then the entry ids are identical across calls
    expect(first.feed.map((entry) => entry.id)).toEqual(
      second.feed.map((entry) => entry.id)
    );
  });

  it('given unknown and step_start lines, when built, then they are ignored and not present in the feed', () => {
    // Given a detail whose lines include unknown, step_start, and step_finish
    const detail = makeDetail({
      runId: 'run-unknown',
      prompt: 'Probe',
      lines: [
        jsonLine({ type: 'step_start' }),
        jsonLine({ type: 'text', text: 'body' }),
        'not even json',
        jsonLine({ type: 'step_finish' }),
        jsonLine({ type: 'unknown', raw: 'garbage' }),
      ],
    });
    // When built
    const result = buildDispatchRunFeed(detail);
    // Then only the user and assistant entries survive; no stray rows from unknown lines
    expect(result.feed.map((entry) => entry.kind)).toEqual([
      'user-message',
      'assistant-message',
    ]);
  });

  it('given a tool_use whose output contains "error", when built, then the work-log status is failed', () => {
    // Given a tool_use line whose output contains "error" (case-insensitive)
    const detail = makeDetail({
      runId: 'run-tool-failed',
      prompt: 'Try',
      lines: [
        jsonLine({
          type: 'tool_use',
          name: 'bash',
          input: { command: 'false' },
          output: 'ERROR: command failed with exit 1',
        }),
      ],
    });
    // When built
    const result = buildDispatchRunFeed(detail);
    // Then the single work-log entry carries a failed status
    const workLog = result.feed.find(
      (entry): entry is Extract<typeof entry, { kind: 'work-log' }> =>
        entry.kind === 'work-log'
    );
    expect(workLog?.entry.status).toBe('failed');
  });

  it('given a tool_use whose output has no error, when built, then the work-log status is done', () => {
    // Given a tool_use line whose output is clean
    const detail = makeDetail({
      runId: 'run-tool-ok',
      prompt: 'Try',
      lines: [
        jsonLine({
          type: 'tool_use',
          name: 'bash',
          input: { command: 'true' },
          output: 'all good',
        }),
      ],
    });
    // When built
    const result = buildDispatchRunFeed(detail);
    // Then the single work-log entry carries a done status
    const workLog = result.feed.find(
      (entry): entry is Extract<typeof entry, { kind: 'work-log' }> =>
        entry.kind === 'work-log'
    );
    expect(workLog?.entry.status).toBe('done');
  });
});