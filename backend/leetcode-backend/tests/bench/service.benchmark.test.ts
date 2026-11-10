/**
 * Service benchmark: the same scripted session against whichever implementation `LEETCODE_BACKEND_CMD` names (default: the release
 * build of the Rust service), one fresh process per run. Per run it measures cold start to the first `/health`, resident memory idle
 * and after the calls, and the latency of each route (p50 of the calls in the scenario). The medians over the runs are written as
 * JSON to `BENCH_OUT`, so a "pre" record and a "post" record come from identical code.
 *
 * Run: `BENCH_LABEL=post BENCH_OUT=<file> pnpm --filter @app/leetcode-backend run bench:service`.
 * Memory is read from `/proc/<pid>/status` (VmRSS now, VmHWM peak), so a command that wraps the service in a shell would measure the
 * shell: name the service binary itself. The service allows 120 requests a minute per caller, so a run stays under 110 calls.
 */
import { spawn } from 'node:child_process';
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { describe, expect, it } from 'vitest';

import { freshSession, json, startService } from './harness.js';

const RUNS = Number(process.env['BENCH_RUNS'] ?? 5);
const LABEL = process.env['BENCH_LABEL'] ?? 'unlabelled';
const OUT = process.env['BENCH_OUT'];
const BINARY = process.env['LEETCODE_BACKEND_CMD'] ?? join(import.meta.dirname, '../../../target/release/leetcode-backend-rs');

const median = (values: number[]): number => {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? (sorted[mid] ?? 0) : ((sorted[mid - 1] ?? 0) + (sorted[mid] ?? 0)) / 2;
};

function memory(pid: number): { rss: number; peak: number } {
  const status = readFileSync(`/proc/${pid}/status`, 'utf8');
  const kb = (key: string) => Number(new RegExp(`${key}:\\s+(\\d+) kB`).exec(status)?.[1] ?? 0);
  return { rss: kb('VmRSS') / 1024, peak: kb('VmHWM') / 1024 };
}

const PYTHON = 'class Solution:\n    def twoSum(self, nums, target):\n        return [0, 1]\n';
const RUN_BODY = JSON.stringify({
  lang: 'python3',
  code: PYTHON,
  methodName: 'twoSum',
  testCases: [{ inputs: { nums: [2, 7], target: 9 }, expected: [0, 1] }],
});
const FORMAT_BODY = JSON.stringify({ lang: 'python3', code: 'x   =  1\n' });

type Run = Record<string, number>;

/** Spawn to the first answered `/health`, polled every millisecond (the harness polls every 25, too coarse for this). */
async function coldStart(port: number): Promise<number> {
  const [cmd = BINARY, ...args] = BINARY.split(' ');
  const spawned = performance.now();
  const child = spawn(cmd, args, {
    env: {
      PATH: process.env['PATH'] ?? '',
      PORT: String(port),
      VM_GATEWAY_AUTH_SEED: 'bench-seed',
      LEETCODE_BACKEND_ADMIN_SECRET: 'bench-admin',
      LEETCODE_CAPTURE_TOOL_BASE_URL: 'http://127.0.0.1:1',
      LEETCODE_CAPTURE_TRIGGER_SECRET: 'bench-capture',
      LEETCODE_BACKEND_SESSION_FILE: `/tmp/lcb-bench-session-${port}.json`,
    },
    stdio: 'ignore',
  });
  try {
    for (;;) {
      if (performance.now() - spawned > 10_000) throw new Error('no answer within 10 s');
      try {
        if ((await fetch(`http://127.0.0.1:${port}/health`)).ok) return performance.now() - spawned;
      } catch {
        await new Promise((resolve) => setTimeout(resolve, 1));
      }
    }
  } finally {
    child.kill('SIGKILL');
    await new Promise((resolve) => child.once('exit', resolve));
  }
}

let nextPort = 21_000 + Math.floor(Math.random() * 2_000);

async function once(): Promise<Run> {
  const coldStartMs = await coldStart(nextPort++);
  const svc = await startService();
  const pid = svc.pid;
  if (pid === undefined) throw new Error('no pid');
  try {
    svc.setSession(freshSession());
    svc.leetcode.reply(json(200, { data: { userStatus: { isSignedIn: true, username: 'bench' } } }));
    await new Promise((resolve) => setTimeout(resolve, 500));
    const idle = memory(pid).rss;

    const time = async (calls: number, call: () => Promise<{ status: number }>): Promise<number> => {
      const durations: number[] = [];
      for (let i = 0; i < calls; i += 1) {
        const started = performance.now();
        const res = await call();
        durations.push(performance.now() - started);
        expect(res.status).toBe(200);
      }
      return median(durations);
    };
    const health = await time(60, () => svc.call('/health', { bearer: false }));
    const afterHealth = memory(pid).rss;
    const userStatus = await time(20, () => svc.call('/api/leetcode/user-status'));
    const run = await time(10, () => svc.call('/api/leetcode/run', { method: 'POST', body: RUN_BODY }));
    const format = await time(10, () => svc.call('/api/leetcode/format', { method: 'POST', body: FORMAT_BODY }));
    const end = memory(pid);
    return {
      coldStartMs,
      idleRssMb: idle,
      rssAfter60HealthMb: afterHealth,
      peakRssMb: end.peak,
      healthMs: health,
      userStatusMs: userStatus,
      runMs: run,
      formatMs: format,
    };
  } finally {
    await svc.stop();
  }
}

describe('leetcode-backend service benchmark', () => {
  it(`given ${RUNS} fresh processes, when the scripted session runs, then the medians are recorded`, async () => {
    const runs: Run[] = [];
    for (let i = 0; i < RUNS; i += 1) runs.push(await once());

    const keys = Object.keys(runs[0] ?? {});
    const medians = Object.fromEntries(keys.map((key) => [key, Number(median(runs.map((r) => r[key] ?? 0)).toFixed(2))]));
    const record = {
      label: LABEL,
      binary: BINARY,
      binaryMb: Number((statSync(BINARY.split(' ')[0] ?? BINARY).size / 1048576).toFixed(2)),
      runs: RUNS,
      medians,
    };
    console.log(JSON.stringify(record));
    if (OUT) {
      mkdirSync(dirname(OUT), { recursive: true });
      writeFileSync(OUT, `${JSON.stringify(record, null, 2)}\n`);
    }
    expect(medians['healthMs']).toBeGreaterThan(0);
  }, 120_000);
});
