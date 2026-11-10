// features/coding-agent/src/testing/node/fakeOpencodeLogServer.mjs
//
// A fake opencode-log HTTP server for e2e (port 8091) and local dev. Plain
// Node ESM, no build step: serves the deterministic dispatch-runs fixtures
// from `node/fixtures.json` (regenerated via export-fixtures.mjs when the
// `dispatchRuns` key is missing). The running fixture gains two text lines
// per request until it has 12 lines, then flips to `finished` like the
// impl-side fixture gateway in contract.md. Status codes and bodies follow
// vm-ops-api's opencode-log reads as an unsigned client sees them; its public
// dashboard pages answer a stub page in place of the dashboard file. HTTP GET
// `/__received` returns every received request as JSON.

import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Buffer } from 'node:buffer';

const here = dirname(fileURLToPath(import.meta.url));
const FIXTURES_PATH = join(here, 'fixtures.json');

const RUNNING_LINES_TARGET = 12;
const RUNNING_LINES_PER_REQUEST = 2;
const FINISHED_COST = 0.0123;
const FINISHED_DURATION_MS = 48210;
const FINISHED_AT = '2026-09-06T10:00:48.210Z';
const RUN_ID_RE = /^[A-Za-z0-9-]{1,100}$/;
const RUN_PATH_PREFIX = '/opencode-log/api/runs/';
const DASHBOARD_PREFIX = '/opencode-log/';
const DASHBOARD_STUB = Buffer.from(
  '<!doctype html><title>opencode-log</title><p>fake opencode-log dashboard</p>\n'
);

function loadFixtures() {
  let raw = readFixturesOrNull();
  if (raw === null || raw.dispatchRuns === undefined) {
    runExportFixtures();
    raw = JSON.parse(readFileSync(FIXTURES_PATH, 'utf8'));
  }
  return { dispatchRuns: raw.dispatchRuns };
}

function readFixturesOrNull() {
  if (!existsSync(FIXTURES_PATH)) return null;
  return JSON.parse(readFileSync(FIXTURES_PATH, 'utf8'));
}

function runExportFixtures() {
  const scriptPath = join(here, 'export-fixtures.mjs');
  const localTsx = join(here, '..', '..', '..', 'node_modules', '.bin', 'tsx');
  const rootTsx = join(here, '..', '..', '..', '..', '..', '..', '..', 'node_modules', '.bin', 'tsx');
  const tsxBin = existsSync(localTsx) ? localTsx : rootTsx;
  execFileSync(tsxBin, [scriptPath], { stdio: 'inherit' });
}

function cloneRun(run) {
  return { ...run, lines: [...run.lines] };
}

function toMeta(run) {
  const { lines, ...meta } = run;
  return meta;
}

function makeRunningLine(index) {
  return JSON.stringify({ type: 'text', text: `Streaming delta ${index}.` });
}

function matchesFilters(run, agent, q) {
  if (agent !== '' && run.agent !== agent) return false;
  return String(run.prompt || '').toLowerCase().includes(q.toLowerCase());
}

// `/opencode-log/api` matches the run-id pattern too, so it is a dashboard page.
function isDashboardPath(pathname) {
  if (pathname === '/opencode-log' || pathname === DASHBOARD_PREFIX) return true;
  return (
    pathname.startsWith(DASHBOARD_PREFIX) &&
    RUN_ID_RE.test(pathname.slice(DASHBOARD_PREFIX.length))
  );
}

function startedMs(run) {
  return new Date(run.startedAt ?? 0).getTime();
}

class FakeOpencodeLogServer {
  constructor({ port = 8091 } = {}) {
    this.port = port;
    this.received = [];
    this.httpServer = null;

    const { dispatchRuns } = loadFixtures();
    this.runs = new Map();
    for (const run of Object.values(dispatchRuns)) {
      this.runs.set(run.runId, cloneRun(run));
    }
  }

  async start() {
    const httpServer = createServer((req, res) => this.handleHttp(req, res));
    this.httpServer = httpServer;
    await new Promise((resolve, reject) => {
      httpServer.once('error', reject);
      httpServer.listen(this.port, () => resolve());
    });
    this.port = httpServer.address().port;
    return this;
  }

  get httpUrl() {
    return `http://localhost:${this.port}`;
  }

  async stop() {
    if (this.httpServer !== null) {
      const server = this.httpServer;
      this.httpServer = null;
      await new Promise((resolve) => server.close(() => resolve()));
    }
  }

  handleHttp(req, res) {
    if (req.method !== 'GET' || typeof req.url !== 'string') {
      this.sendJson(res, 401, { error: 'unauthorized' });
      return;
    }
    this.recordReceived(req);
    let url;
    try {
      url = new URL(req.url, 'http://localhost');
    } catch (err) {
      this.sendJson(res, 500, { error: String(err?.message ?? err) });
      return;
    }
    const { pathname, searchParams } = url;

    if (pathname === '/__received') {
      this.sendJson(res, 200, this.received);
      return;
    }

    if (pathname === '/opencode-log/api/runs') {
      const agent = searchParams.get('agent') || '';
      const q = searchParams.get('q') || '';
      const metas = Array.from(this.runs.values())
        .filter((run) => matchesFilters(run, agent, q))
        .sort((a, b) => startedMs(b) - startedMs(a))
        .map(toMeta);
      this.sendJson(res, 200, { runs: metas });
      return;
    }

    if (pathname.startsWith(RUN_PATH_PREFIX)) {
      const runId = pathname.slice(RUN_PATH_PREFIX.length);
      const run = RUN_ID_RE.test(runId) ? this.runs.get(runId) : undefined;
      if (run === undefined) {
        this.sendJson(res, 404, { error: 'run not found' });
        return;
      }
      this.advanceRunningRun(run);
      this.sendJson(res, 200, cloneRun(run));
      return;
    }

    if (isDashboardPath(pathname)) {
      res.writeHead(200, {
        'content-type': 'text/html; charset=utf-8',
        'content-length': DASHBOARD_STUB.length,
      });
      res.end(DASHBOARD_STUB);
      return;
    }

    // vm-ops-api answers every other path 401 to a request without its bearer.
    this.sendJson(res, 401, { error: 'unauthorized' });
  }

  recordReceived(req) {
    this.received.push({ method: req.method, url: req.url });
  }

  advanceRunningRun(run) {
    if (run.status !== 'running') return;
    if (run.lines.length >= RUNNING_LINES_TARGET) {
      this.finalizeRunningRun(run);
      return;
    }
    for (let i = 0; i < RUNNING_LINES_PER_REQUEST; i += 1) {
      run.lines.push(makeRunningLine(run.lines.length + 1));
    }
    if (run.lines.length >= RUNNING_LINES_TARGET) {
      this.finalizeRunningRun(run);
    }
  }

  finalizeRunningRun(run) {
    run.status = 'finished';
    run.finishedAt = FINISHED_AT;
    run.ok = true;
    run.cost = FINISHED_COST;
    run.durationMs = FINISHED_DURATION_MS;
  }

  sendJson(res, status, body) {
    res.writeHead(status, {
      'content-type': 'application/json',
      // The real service never sends this; the web e2e serves the export and
      // this fake (8091) on different ports, so the app's fetch is cross-origin.
      'access-control-allow-origin': '*',
    });
    res.end(JSON.stringify(body));
  }
}

export async function startFakeOpencodeLogServer(options = {}) {
  const server = new FakeOpencodeLogServer(options);
  await server.start();
  return server;
}
