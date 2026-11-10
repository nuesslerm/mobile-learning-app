/**
 * d13-e2e-runs-live: prove the dispatch-run detail screen tails a running
 * fixture in real time and flips to a finished header when the fake server
 * stops appending lines.
 *
 * Boots startFakeWsAgentServer on 8787 (so the FlatList populates with
 * threads and the DispatchRunsSection footer mounts) and
 * startFakeOpencodeLogServer on 8091 (the HTTP gateway baked via
 * EXPO_PUBLIC_VM_OPS_API_BASE_URL). Opens /coding-agent, taps the
 * running row to navigate client-side into the run detail screen (the
 * static export does not serve a parameterized URL for /[runId], so
 * direct visits 404 -- routing through the in-app router avoids that),
 * waits for DispatchRunFeed to render, samples its inner text length,
 * waits 2000 ms, samples again and asserts it grew (the fake server
 * adds two lines per poll so the text must grow across a 1.5 s poll),
 * asserts the WorkingIndicator (testID 'WorkingIndicator') is visible
 * while the run is still running, polls the header text up to 15 s for
 * '$0.0123' (the cost the fake server writes when it finalises the run
 * after 12 lines) and asserts the WorkingIndicator is gone once the
 * header carries the cost, then asserts /__received on the fake server
 * recorded at least 3 GETs for /opencode-log/api/runs/fx-run-running.
 * Exits 1 on any failure with a FAIL: line.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';
import { startFakeWsAgentServer } from '../features/coding-agent/src/testing/node/fakeWsAgentServer.mjs';
import { startFakeOpencodeLogServer } from '../features/coding-agent/src/testing/node/fakeOpencodeLogServer.mjs';

const DISPATCH_RUNS_SECTION = '[data-testid="DispatchRunsSection"]';
const RUNNING_ROW = '[data-testid="DispatchRunRow-fx-run-running"]';
const DISPATCH_RUN_SCREEN = '[data-testid="DispatchRunScreen"]';
const DISPATCH_RUN_HEADER = '[data-testid="DispatchRunHeader"]';
const DISPATCH_RUN_FEED = '[data-testid="DispatchRunFeed"]';
const WORKING_INDICATOR = '[data-testid="WorkingIndicator"]';

const FATAL_ERROR_RE = /Minified React error|isReady/;

const RUN_ID = 'fx-run-running';
const FINISHED_COST_TEXT = '$0.0123';
const GROWTH_GAP_MS = 2000;
const RUNNING_SETTLE_MS = 500;
const FEED_TEXT_MIN_LEN = 50;
const FINISH_POLL_BUDGET_MS = 15000;
const FINISH_POLL_INTERVAL_MS = 500;
const MIN_RUN_DETAIL_GETS = 3;

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
    { port: 8207, name: 'coding-agent-runs-live-verify', path: 'coding-agent' },
    async ({ evalInApp, tapInApp }) => {
      // ---- Given: DispatchRunsSection mounted and the running row is in view
      // FlatList only mounts its footer once it has data, so this loop also
      // waits for the WS handshake to populate threads.
      let sectionView = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        await sleep(500);
        sectionView = await evalInApp(`(() => {
          const section = appDoc.querySelector(${JSON.stringify(DISPATCH_RUNS_SECTION)});
          const running = appDoc.querySelector(${JSON.stringify(RUNNING_ROW)});
          const runningRect = running ? running.getBoundingClientRect() : null;
          return {
            sectionPresent: !!section,
            runningHeight: runningRect ? runningRect.height : 0,
            vh: appDoc.defaultView.innerHeight,
          };
        })()`).catch(() => null);
        if (sectionView?.sectionPresent && sectionView.runningHeight > 0) break;
      }
      if (!sectionView?.sectionPresent || sectionView.runningHeight <= 0) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `DispatchRunsSection / DispatchRunRow-${RUN_ID} never appeared: ` +
            `${JSON.stringify(sectionView)} pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }

      // ---- When the running row is tapped -------------------------------
      // Scroll it into view first (the FlatList scroller on iphone-15-pro
      // pushes it under the keyboard area), then tap. expo-router transitions
      // client-side to /coding-agent/runs/fx-run-running, which the static
      // export does not serve directly but the in-app router renders fine.
      await evalInApp(`(() => {
        const el = appDoc.querySelector(${JSON.stringify(RUNNING_ROW)});
        if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center' });
        return !!el;
      })()`).catch(() => null);
      await sleep(300);
      await tapInApp(RUNNING_ROW);

      // ---- Then DispatchRunScreen mounts and the feed starts rendering ---
      // Wait until the screen, feed, header are all present and the feed
      // carries some non-trivial text -- this is the running baseline.
      let baseline = null;
      for (let attempt = 0; attempt < 20; attempt++) {
        await sleep(RUNNING_SETTLE_MS);
        baseline = await evalInApp(`(() => {
          const screen = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_SCREEN)});
          const header = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_HEADER)});
          const feed = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_FEED)});
          const working = appDoc.querySelector(${JSON.stringify(WORKING_INDICATOR)});
          return {
            path: appDoc.location.pathname,
            screenPresent: !!screen,
            feedPresent: !!feed,
            feedTextLength: feed ? feed.textContent.length : 0,
            workingPresent: !!working,
            headerText: header ? header.textContent : '',
          };
        })()`).catch(() => null);
        if (
          baseline?.path.endsWith(`/coding-agent/runs/${RUN_ID}`) &&
          baseline.screenPresent &&
          baseline.feedPresent &&
          baseline.feedTextLength > FEED_TEXT_MIN_LEN &&
          baseline.workingPresent
        ) {
          break;
        }
      }
      if (
        !baseline?.screenPresent ||
        !baseline.feedPresent ||
        baseline.feedTextLength <= FEED_TEXT_MIN_LEN
      ) {
        const pageErrors = await readPageErrors(evalInApp);
        fail(
          `tapping ${RUNNING_ROW} did not mount DispatchRunScreen + DispatchRunFeed ` +
            `with running state: ${JSON.stringify(baseline)} pageErrors=${JSON.stringify(pageErrors)}`,
        );
        return;
      }

      // ---- While running: the WorkingIndicator must be visible ----------
      if (!baseline.workingPresent) {
        fail(
          `${WORKING_INDICATOR} not visible while run is still running: ` +
            `${JSON.stringify(baseline)}`,
        );
        return;
      }

      const lengthBefore = baseline.feedTextLength;
      const headerBefore = baseline.headerText;

      // ---- Wait GROWTH_GAP_MS and re-sample the feed's inner text length -
      // The detail hook polls every 1500 ms while status === 'running', and
      // each poll adds two text lines; GROWTH_GAP_MS (2000) is wide enough to
      // guarantee at least one new poll lands in the window.
      await sleep(GROWTH_GAP_MS);
      const grown = await evalInApp(`(() => {
        const feed = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_FEED)});
        const working = appDoc.querySelector(${JSON.stringify(WORKING_INDICATOR)});
        return {
          feedTextLength: feed ? feed.textContent.length : 0,
          workingPresent: !!working,
        };
      })()`).catch(() => null);
      const lengthAfter = grown?.feedTextLength ?? 0;
      if (lengthAfter <= lengthBefore) {
        fail(
          `DispatchRunFeed text did not grow over ${GROWTH_GAP_MS} ms ` +
            `(before=${lengthBefore}, after=${lengthAfter}): ${JSON.stringify(grown)}`,
        );
        return;
      }
      if (!grown?.workingPresent) {
        fail(
          `${WORKING_INDICATOR} disappeared before the run flipped to finished ` +
            `(lengthBefore=${lengthBefore}, lengthAfter=${lengthAfter}): ${JSON.stringify(grown)}`,
        );
        return;
      }

      // ---- Poll the header up to FINISH_POLL_BUDGET_MS for FINISHED_COST_TEXT -
      // The fake server finalises the run when lines.length >= 12; the header
      // switches from the 'running' subtitle to 'tokens · cost · duration' the
      // moment the store sees status === 'finished'. Once the cost lands, the
      // working entry must be gone too.
      let finished = null;
      const pollCount = Math.ceil(FINISH_POLL_BUDGET_MS / FINISH_POLL_INTERVAL_MS);
      for (let attempt = 0; attempt < pollCount; attempt++) {
        await sleep(FINISH_POLL_INTERVAL_MS);
        finished = await evalInApp(`(() => {
          const header = appDoc.querySelector(${JSON.stringify(DISPATCH_RUN_HEADER)});
          const working = appDoc.querySelector(${JSON.stringify(WORKING_INDICATOR)});
          return {
            headerText: header ? header.textContent : '',
            workingPresent: !!working,
          };
        })()`).catch(() => null);
        if (
          finished?.headerText.includes(FINISHED_COST_TEXT) &&
          !finished.workingPresent
        ) {
          break;
        }
      }
      if (!finished?.headerText.includes(FINISHED_COST_TEXT)) {
        fail(
          `${DISPATCH_RUN_HEADER} never carried '${FINISHED_COST_TEXT}' within ` +
            `${FINISH_POLL_BUDGET_MS} ms: headerBefore=${JSON.stringify(headerBefore)} ` +
            `headerLast=${JSON.stringify(finished?.headerText)} workingPresent=${finished?.workingPresent}`,
        );
        return;
      }
      if (finished.workingPresent) {
        fail(
          `${WORKING_INDICATOR} still visible after the run flipped to finished ` +
            `(header='${finished.headerText}'): ${JSON.stringify(finished)}`,
        );
        return;
      }

      // ---- (1) When the run was tailed from running through to finished -
      // Then /__received on the opencode-log fake records at least
      // MIN_RUN_DETAIL_GETS GETs for /opencode-log/api/runs/fx-run-running.
      let received = [];
      try {
        received = await fetchReceived(logServer);
      } catch (err) {
        fail(
          `/__received could not be read from opencode-log server: ${err?.message ?? err}`,
        );
        return;
      }
      const detailCalls = received.filter(
        (r) =>
          r &&
          r.method === 'GET' &&
          typeof r.url === 'string' &&
          r.url.endsWith(`/opencode-log/api/runs/${RUN_ID}`),
      );
      if (detailCalls.length < MIN_RUN_DETAIL_GETS) {
        fail(
          `expected at least ${MIN_RUN_DETAIL_GETS} GET ` +
            `/opencode-log/api/runs/${RUN_ID} on the wire, saw ${detailCalls.length}: ` +
            `${JSON.stringify(received)}`,
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
        `ok: tapping ${RUNNING_ROW} mounted DispatchRunScreen at ${baseline.path}; ` +
          `DispatchRunFeed inner text length grew from ${lengthBefore} to ${lengthAfter} ` +
          `chars across ${GROWTH_GAP_MS} ms while ${WORKING_INDICATOR} stayed visible; ` +
          `after polling for up to ${FINISH_POLL_BUDGET_MS} ms the ` +
          `${DISPATCH_RUN_HEADER} carried '${FINISHED_COST_TEXT}' and ${WORKING_INDICATOR} ` +
          `was gone (final header='${finished.headerText}'); the opencode-log fake ` +
          `recorded ${detailCalls.length} GET /opencode-log/api/runs/${RUN_ID} call(s); ` +
          `no Minified-React-error/isReady page errors.`,
      );
    }
  );
} finally {
  await logServer.stop();
  await wsServer.stop();
}
