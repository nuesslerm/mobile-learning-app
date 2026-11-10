import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { transform } from '@babel/core';
import { describe, expect, it } from 'vitest';

import injectLoggerSource from '../../src/babel/injectLoggerSource';

const REPO_ROOT = resolve(__dirname, '../../../../..');
const SOURCE = 'frontend/libraries/logging/tests/fixtures/callsite.ts';
const CONTRACT = 'clis/telemetry/contract/callsite.json';

// The fixture, compiled by the real Babel plugin, run against a Logger that records the context the plugin injected, one JSON
// line per call: the record a call site produces, in the keys the contract names for `ts`.
async function recordsOfTheFixture(): Promise<string[]> {
  const result = transform(readFileSync(resolve(REPO_ROOT, SOURCE), 'utf8'), {
    filename: SOURCE,
    cwd: REPO_ROOT,
    configFile: false,
    babelrc: false,
    presets: [['@babel/preset-typescript', { allExtensions: true }]],
    plugins: [injectLoggerSource],
  });
  const code = (result?.code ?? '')
    .replace(/^import .*$/gm, '')
    .replace(/^export /gm, '');
  const lines: string[] = [];
  const Logger = {
    withContext: (context: Record<string, unknown>) => ({
      info: (msg: string) => lines.push(JSON.stringify({ msg, ...context })),
    }),
  };
  const run = new Function('Logger', `${code}\nreturn runAll;`)(Logger) as () => Promise<void>;
  await run();
  return lines;
}

// The black box is the Rust package's `callsite-check`, which derives the expected file, line and function from the fixture's
// `@site` markers and compares them with the records.
// Asynchronous: a cold `cargo run` takes over a minute, and a blocked event loop makes vitest's worker miss its RPC deadline.
function check(records: string[]): Promise<{ status: number | null; verdict: { ok: boolean; cases: { case: string; ok: boolean }[]; problems: string[] } }> {
  const bin = process.env.CALLSITE_CHECK_BIN;
  const [command, ...args] = bin
    ? [bin]
    : ['cargo', 'run', '--quiet', '--locked', '--manifest-path', 'clis/telemetry/Cargo.toml', '-p', 'telemetry', '--features', 'callsite', '--bin', 'callsite-check', '--'];
  return new Promise((done, fail) => {
    const run = spawn(command, [...args, '--contract', CONTRACT, '--impl', 'ts', '--source', SOURCE, '--repo-root', '.'], { cwd: REPO_ROOT });
    let stdout = '';
    run.stdout.setEncoding('utf8').on('data', (chunk: string) => (stdout += chunk));
    run.on('error', fail);
    run.on('close', (status) => done({ status, verdict: JSON.parse(stdout.trim().split('\n').pop() ?? '{}') }));
    run.stdin.end(records.join('\n'));
  });
}

describe('the TypeScript logger against the call-site contract', () => {
  it('given the marked fixture, when its records are checked, then every case names its file, line and function', async () => {
    const { status, verdict } = await check(await recordsOfTheFixture());

    expect(verdict.problems).toEqual([]);
    expect(verdict.cases.filter((c) => !c.ok)).toEqual([]);
    expect(verdict.cases).toHaveLength(7);
    expect(status).toBe(0);
  }, 300_000);

  it('given a record whose line is off by one, when checked, then the case fails and the check exits 1', async () => {
    const records = (await recordsOfTheFixture()).map((l) => {
      const record = JSON.parse(l) as { msg: string; line: number };
      return JSON.stringify(record.msg === 'nested-function' ? { ...record, line: record.line + 1 } : record);
    });
    const { status, verdict } = await check(records);

    expect(verdict.cases.filter((c) => !c.ok).map((c) => c.case)).toEqual(['nested-function']);
    expect(status).toBe(1);
  }, 300_000);
});
