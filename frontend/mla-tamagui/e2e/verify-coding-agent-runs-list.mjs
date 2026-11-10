/**
 * d11-e2e-runs-list: prove the dispatch-runs section lists the fake
 * opencode-log runs, surfaces the right rows, and navigates into the run
 * detail screen.
 *
 * Boots two fake servers: startFakeWsAgentServer on 8787 (so
 * ThreadListScreen's FlatList mounts with rows -- DispatchRunsSection is the
 * FlatList footer, so it is only mounted once the threads populate) and
 * startFakeOpencodeLogServer on 8091 (the HTTP gateway baked via
 * EXPO_PUBLIC_VM_OPS_API_BASE_URL).
 *
 * Asserts geometry (> 40 px row height), text content (finished row carries
 * 'implementer' and '$0.0123', running row carries 'researcher'), the
 * fake server's recorded GET on /opencode-log/api/runs, and row-tap
 * navigation. Exits 1 on any failure with a FAIL: line.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';
import { startFakeOpencodeLogServer } from '../features/coding-agent/src/testing/node/fakeOpencodeLogServer.mjs';

const DISPATCH_RUNS_SECTION = '[data-testid="DispatchRunsSection"]';
const FINISHED_ROW = '[data-testid="DispatchRunRow-fx-run-finished"]';
const RUNNING_ROW = '[data-testid="DispatchRunRow-fx-run-running"]';

const FATAL_ERROR_RE = /Minified React error|isReady/;

const wsServer = await startFakeWsAgentServer({ port: 8787 });
const logServer = await startFakeOpencodeLogServer({ port: 8091 });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function fetchReceived(server) {
  const res = await fetch(`${server.httpUrl}/__received`);
  if (!res.ok) throw new Error(`/__received returned ${res.status}`);
  return res.json();
}

async function readPageErrors(evalInApp) {
  const raw = await evalInApp(
    `JSON.stringify((appDoc.defaultView && appDoc.defaultView.__pageErrors) || [])`,
  ).catch(() => '[]');
  const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
  return Array.isArray(parsed) ? parsed : [];
}

try {
  await withServedApp(
    { port: 8205, name: 'coding-agent-runs-list-verify', path: 'coding-agent', video: true },
    async ({ evalInApp, tapInApp }) => {
      // ---- Given: the section is mounted and both rows have text content -
      // FlatList only mounts its footer once its data is non-empty, so this
      // loop also implicitly waits for the WS handshake to populate threads.
      let runsView = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        await sleep(500);
        runsView = await evalInApp(`(() => {
          const section = appDoc.querySelector(${JSON.stringify(DISPATCH_RUNS_SECTION)});
          const finished = appDoc.querySelector(${JSON.stringify(FINISHED_ROW)});
          const running = appDoc.querySelector(${JSON.stringify(RUNNING_ROW)});
          const finishedRect = finished ? finished.getBoundingClientRect() : null;
          const runningRect = running ? running.getBoundingClientRect() : null;
          return {
            sectionPresent: !!section,
            finishedHeight: finishedRect ? finishedRect.height : 0,
            runningHeight: runningRect ? runningRect.height : 0,
            finishedText: finished ? finished.textContent : '',
            runningText: running ? running.textContent : '',
          };
        })()`).catch(() => null);
        if (
          runsView?.sectionPresent &&
          runsView.finishedHeight > 0 &&
          runsView.runningHeight > 0
        ) {
          break;
        }
      }

      if (!runsView?.sectionPresent) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `DispatchRunsSection never mounted: ${JSON.stringify(runsView)} ` +
            `pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }

      // ---- (1) Then both rows have non-trivial geometry and the right text
      if (runsView.finishedHeight <= 40 || runsView.runningHeight <= 40) {
        fail(
          `dispatch rows are too short (expected > 40 px): ` +
            `finished=${runsView.finishedHeight} running=${runsView.runningHeight}`,
        );
        return;
      }
      if (!runsView.finishedText.includes('implementer')) {
        fail(
          `finished row text does not contain 'implementer': ` +
            `${JSON.stringify(runsView.finishedText)}`,
        );
        return;
      }
      if (!runsView.finishedText.includes('$0.0123')) {
        fail(
          `finished row text does not contain '$0.0123': ` +
            `${JSON.stringify(runsView.finishedText)}`,
        );
        return;
      }
      if (!runsView.runningText.includes('researcher')) {
        fail(
          `running row text does not contain 'researcher': ` +
            `${JSON.stringify(runsView.runningText)}`,
        );
        return;
      }

      // ---- (2) Then the opencode-log fake recorded a list call ------------
      let received = [];
      try {
        received = await fetchReceived(logServer);
      } catch (err) {
        fail(
          `/__received could not be read from opencode-log server: ${err?.message ?? err}`,
        );
        return;
      }
      const listCalls = received.filter(
        (r) =>
          r &&
          r.method === 'GET' &&
          typeof r.url === 'string' &&
          r.url.startsWith('/opencode-log/api/runs'),
      );
      if (listCalls.length === 0) {
        fail(
          `expected at least one GET /opencode-log/api/runs on the wire, ` +
            `saw: ${JSON.stringify(received)}`,
        );
        return;
      }

      // ---- (3) When the finished row is tapped ---------------------------
      // Then the URL path ends with /coding-agent/runs/fx-run-finished.
      // The row is the FlatList footer: with the device-frame viewport (852px
      // tall) and 10 fixture thread rows on top, the row's centre lands below
      // the viewport and elementFromPoint returns null. Scroll it into view
      // first; scrollIntoView on web walks every scroll ancestor, which is the
      // FlatList's inner scroller.
      await evalInApp(`(() => {
        const el = appDoc.querySelector(${JSON.stringify(FINISHED_ROW)});
        if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center' });
        return !!el;
      })()`).catch(() => null);
      await sleep(300);
      await tapInApp(FINISHED_ROW);
      let navigated = null;
      for (let attempt = 0; attempt < 10; attempt++) {
        navigated = await evalInApp(`(() => ({
          path: appDoc.location.pathname,
        }))()`).catch(() => null);
        if (navigated?.path.endsWith('/coding-agent/runs/fx-run-finished')) break;
        await sleep(400);
      }
      if (!navigated?.path.endsWith('/coding-agent/runs/fx-run-finished')) {
        fail(
          `tapping the finished row did not navigate to ` +
            `/coding-agent/runs/fx-run-finished: ${JSON.stringify(navigated)}`,
        );
        return;
      }

      // ---- No fatal page errors may survive the run -----------------------
      const pageErrors = await readPageErrors(evalInApp);
      const fatal = pageErrors.filter((e) => FATAL_ERROR_RE.test(e));
      if (fatal.length > 0) {
        fail(`fatal page errors at end of run: ${JSON.stringify(fatal)}`);
        return;
      }

      console.log(
        `ok: DispatchRunsSection mounted with finished row ` +
          `(height=${runsView.finishedHeight}px, text contains 'implementer' and ` +
          `'$0.0123') and running row (height=${runsView.runningHeight}px, text ` +
          `contains 'researcher'); the opencode-log fake recorded ${listCalls.length} ` +
          `GET /opencode-log/api/runs call(s); tapping the finished row navigated ` +
          `to ${navigated.path}; no Minified-React-error/isReady page errors.`,
      );
    }
  );
} finally {
  await logServer.stop();
  await wsServer.stop();
}
