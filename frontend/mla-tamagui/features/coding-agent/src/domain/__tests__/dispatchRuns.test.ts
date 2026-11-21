import { describe, expect, it } from 'vitest';

import {
  DispatchRunMeta,
  DispatchRunLine,
  DispatchRunsListResponse,
  parseDispatchRunLine,
} from '../dispatchRuns';

describe('DispatchRunMeta', () => {
  it('given a finished run, when parsed, then every field round-trips', () => {
    // Given a finished run meta with cost, tokens and a finishedAt
    const raw = {
      runId: 'run-abc-123',
      prompt: 'refactor the auth middleware',
      agent: 'opencode',
      caller: 'mobile',
      claudeSessionId: 'sess-9',
      model: 'sonnet',
      startedAt: '2026-09-06T10:00:00.000Z',
      finishedAt: '2026-09-06T10:01:30.000Z',
      status: 'finished' as const,
      ok: true,
      tokens: { total: 1234, input: 800, output: 434 },
      cost: 0.0123,
      durationMs: 90_000,
    };
    // When parsed
    const result = DispatchRunMeta.safeParse(raw);
    // Then every field round-trips
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data).toEqual(raw);
    }
  });

  it('given a running run with null finishedAt and cost, when parsed, then the nullable fields stay null', () => {
    // Given a running meta whose finishedAt, ok and cost are null
    const raw = {
      runId: 'run-running-1',
      prompt: 'still working',
      startedAt: '2026-09-06T10:00:00.000Z',
      finishedAt: null,
      status: 'running' as const,
      ok: null,
      tokens: null,
      cost: null,
      durationMs: null,
    };
    // When parsed
    const result = DispatchRunMeta.safeParse(raw);
    // Then the nullables stay null and the run is accepted
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.finishedAt).toBeNull();
      expect(result.data.cost).toBeNull();
      expect(result.data.durationMs).toBeNull();
      expect(result.data.tokens).toBeNull();
      expect(result.data.ok).toBeNull();
      expect(result.data.status).toBe('running');
    }
  });
});

describe('DispatchRunLine', () => {
  it('given a tool_use line, when parsed, then name and input round-trip and output stays undefined', () => {
    // Given a tool_use line with input only
    const raw = {
      type: 'tool_use',
      name: 'edit_file',
      input: { path: 'src/auth.ts', oldString: 'a', newString: 'b' },
    };
    // When parsed
    const result = DispatchRunLine.safeParse(raw);
    // Then the name and input round-trip and output stays optional
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data).toEqual({
        type: 'tool_use',
        name: 'edit_file',
        input: { path: 'src/auth.ts', oldString: 'a', newString: 'b' },
      });
    }
  });
});

describe('parseDispatchRunLine', () => {
  it('given malformed JSON, when parsed, then an unknown line with the raw text is returned', () => {
    // Given a line that is not JSON
    const raw = 'not-json-at-all{';
    // When parsed
    const line = parseDispatchRunLine(raw);
    // Then it collapses to unknown with the original text preserved
    expect(line).toEqual({ type: 'unknown', raw });
  });

  it('given a JSON object with an unknown type, when parsed, then an unknown line is returned', () => {
    // Given a JSON object whose type the union does not know
    const raw = JSON.stringify({ type: 'something_new', payload: 1 });
    // When parsed
    const line = parseDispatchRunLine(raw);
    // Then it collapses to unknown and keeps the raw line
    expect(line).toEqual({ type: 'unknown', raw });
  });
});

describe('DispatchRunsListResponse', () => {
  it('given a list payload with an extra unknown field, when parsed, then the runs array still parses', () => {
    // Given a list response with one run plus an unknown top-level key
    const raw = {
      runs: [
        {
          runId: 'run-1',
          prompt: 'p',
          startedAt: '2026-09-06T10:00:00.000Z',
          finishedAt: null,
          status: 'finished' as const,
          ok: true,
          tokens: { total: 1, input: 1, output: 0 },
          cost: 0.0,
          durationMs: 10,
        },
      ],
      nextCursor: 'abc',
    };
    // When parsed
    const result = DispatchRunsListResponse.safeParse(raw);
    // Then the runs array parses and the extra field is ignored
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.runs).toHaveLength(1);
      expect(result.data.runs[0].runId).toBe('run-1');
    }
  });
});
