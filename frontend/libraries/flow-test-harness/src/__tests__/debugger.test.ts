import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, describe, expect, it, vi } from 'vitest';

import { breakAt, logAt } from '../debugger';

import { totalQuantity } from './fixtures/debugTarget';

const TARGET = 'src/__tests__/fixtures/debugTarget.ts';
const ADD_LINE = 6;

describe('flow debugger probes', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('Given a logpoint on the line that adds a quantity, When the function runs over three lines, Then each pass is recorded with its locals and an original-line stack, and the result is unchanged', () => {
    // Given a logpoint at the accumulating line, writing to a fresh debug file
    const out = path.join(
      mkdtempSync(path.join(tmpdir(), 'flow-debug-')),
      'debug.jsonl'
    );
    vi.stubEnv('FLOW_DEBUG_OUT', out);
    const probe = logAt(TARGET, ADD_LINE, ['line.sku', 'total']);

    // When the code under test runs
    let result: number;
    try {
      result = totalQuantity([
        { sku: 'a', qty: 1 },
        { sku: 'b', qty: 2 },
        { sku: 'c', qty: 4 },
      ]);
    } finally {
      probe.release();
    }

    // Then every pass was recorded at the original line, the code carried on, and the stops landed in the file
    expect(result).toBe(7);
    expect(probe.stops.map((s) => s.values)).toEqual([
      { 'line.sku': 'a', total: 0 },
      { 'line.sku': 'b', total: 1 },
      { 'line.sku': 'c', total: 3 },
    ]);
    const [first] = probe.stops;
    expect(first).toMatchObject({
      kind: 'logpoint',
      file: TARGET,
      line: ADD_LINE,
      function: 'totalQuantity',
      testFile: 'src/__tests__/debugger.test.ts',
    });
    expect(first.test).toContain('Given a logpoint');
    expect(first.stack[0]).toEqual({
      function: 'totalQuantity',
      file: TARGET,
      line: ADD_LINE,
    });
    expect(
      first.stack.some((f) => f.file === 'src/__tests__/debugger.test.ts')
    ).toBe(true);
    const lines = readFileSync(out, 'utf8')
      .trim()
      .split('\n')
      .map((l) => JSON.parse(l));
    expect(lines).toHaveLength(3);
    expect(lines[2]).toMatchObject({ kind: 'logpoint', values: { total: 3 } });
  });

  it('Given a breakpoint with an explicit root, When the line runs once, Then the stop is reported relative to that root', () => {
    // Given a root one level above the harness's own, so the file path carries the package directory
    const packageDir = path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      '../..'
    );
    const root = path.dirname(packageDir);
    const file = path.join(path.basename(packageDir), TARGET);
    const scoped = breakAt(file, ADD_LINE, ['line.qty'], { root });

    // When the line runs once
    try {
      totalQuantity([{ sku: 'z', qty: 9 }]);
    } finally {
      scoped.release();
    }

    // Then the stop carries the root-relative file and the read value, and its test file stays project-relative
    expect(scoped.stops).toEqual([
      expect.objectContaining({
        kind: 'breakpoint',
        file,
        line: ADD_LINE,
        values: { 'line.qty': 9 },
        testFile: 'src/__tests__/debugger.test.ts',
      }),
    ]);
    expect(scoped.stops[0].stack[0]).toEqual({
      function: 'totalQuantity',
      file,
      line: ADD_LINE,
    });
  });

  it('Given a file the test never imported, When a breakpoint is set there, Then it fails naming the file', () => {
    // Given / When / Then
    expect(() => breakAt('src/doubles/fakeAudioRecorder.ts', 1, [])).toThrow(
      /fakeAudioRecorder\.ts is not loaded/
    );
  });
});
